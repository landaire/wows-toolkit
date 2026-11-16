//! The textures a scene's hull materials reference, one entry per material with the meshes that
//! draw it, and the files in the packages that hold each texture. Built on demand: it reads one
//! `.dds` header per texture.

use std::collections::BTreeMap;
use std::collections::HashMap;

use wowsunpack::export::gltf_export::InteractiveHullMesh;
use wowsunpack::export::material_textures::PropertyName;
use wowsunpack::export::material_textures::TextureFiles;
use wowsunpack::export::material_textures::TextureFilesError;
use wowsunpack::export::material_textures::TextureReference;
use wowsunpack::export::material_textures::texture_files;
use wowsunpack::export::model_library::ModelLibrary;

use crate::hull_picking::HullMeshIndex;

/// Every material of a scene's hull meshes, ordered by `.mfm` path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneTextures {
    materials: Vec<SceneMaterial>,
}

impl SceneTextures {
    /// Groups `meshes` by material and looks up what each material references in `library`,
    /// which must be over the build the meshes were loaded from.
    pub fn build(library: &ModelLibrary, meshes: &[InteractiveHullMesh]) -> Self {
        let mut grouped: BTreeMap<(Option<&str>, u64), Vec<HullMeshIndex>> = BTreeMap::new();
        for (index, mesh) in meshes.iter().enumerate() {
            grouped.entry((mesh.mfm_path.as_deref(), mesh.mfm_path_id)).or_default().push(HullMeshIndex::new(index));
        }

        let mut files = FileCache::default();
        let materials = grouped
            .into_iter()
            .map(|((mfm_path, mfm_path_id), indices)| {
                let mut mesh_names: Vec<String> =
                    indices.iter().map(|index| meshes[index.get()].name.clone()).collect();
                mesh_names.sort();
                mesh_names.dedup();

                // A mesh with no material carries id 0, which no record has.
                let properties =
                    (mfm_path_id != 0).then(|| library.material_textures(mfm_path_id)).flatten().map(|refs| {
                        refs.into_iter()
                            .map(|reference| PropertyTexture {
                                property: reference.property,
                                texture: match reference.texture {
                                    TextureReference::Path(path) => TextureLookup::Found(files.located(library, path)),
                                    TextureReference::Unresolved(path_id) => TextureLookup::Unresolved(path_id),
                                },
                            })
                            .collect::<Vec<_>>()
                    });

                let diffuse_present = properties.iter().flatten().any(|texture| {
                    texture.property == PropertyName::Known("diffuseMap")
                        && matches!(&texture.texture, TextureLookup::Found(located) if located.files.is_ok())
                });
                let name_matched_albedo = match (diffuse_present, mfm_path) {
                    (false, Some(mfm_path)) => {
                        library.name_matched_albedo(mfm_path).map(|path| files.located(library, path))
                    }
                    (true, _) | (false, None) => None,
                };

                SceneMaterial {
                    mfm_path: mfm_path.map(|path| path.trim_start_matches('/').to_string()),
                    meshes: indices,
                    mesh_names,
                    properties,
                    name_matched_albedo,
                }
            })
            .collect();

        Self { materials }
    }

    /// An inventory of `materials` as given, for a host's tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn from_materials(materials: Vec<SceneMaterial>) -> Self {
        Self { materials }
    }

    pub fn materials(&self) -> &[SceneMaterial] {
        &self.materials
    }

    /// The position in [`Self::materials`] of the material `mesh` draws with.
    pub fn material_of(&self, mesh: HullMeshIndex) -> Option<usize> {
        self.materials.iter().position(|material| material.meshes.contains(&mesh))
    }
}

/// One material and the meshes drawn with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneMaterial {
    /// VFS path of the `.mfm`, without a leading slash. `None` for meshes the visual names no
    /// material for.
    pub mfm_path: Option<String>,
    pub meshes: Vec<HullMeshIndex>,
    /// The names of `meshes`, sorted, each once.
    pub mesh_names: Vec<String>,
    /// The material's texture properties, in record order. `None` when its record does not read.
    pub properties: Option<Vec<PropertyTexture>>,
    /// The albedo the viewer finds by the material's file name, when no `diffuseMap` names a file
    /// in the packages.
    pub name_matched_albedo: Option<LocatedTexture>,
}

impl SceneMaterial {
    /// Every texture found in the packages, the name-matched albedo last.
    pub fn located(&self) -> impl Iterator<Item = &LocatedTexture> {
        let properties = self.properties.iter().flatten().filter_map(|texture| match &texture.texture {
            TextureLookup::Found(located) => Some(located),
            TextureLookup::Unresolved(_) => None,
        });

        properties.chain(self.name_matched_albedo.as_ref())
    }
}

/// One texture-valued property of a material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyTexture {
    pub property: PropertyName,
    pub texture: TextureLookup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextureLookup {
    Found(LocatedTexture),
    /// A path id the `assets.bin` path store does not hold.
    Unresolved(u64),
}

/// A texture's VFS path and the files holding it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatedTexture {
    /// Without a leading slash.
    pub path: String,
    pub files: Result<TextureFiles, TextureFilesError>,
}

/// Each texture's files, read once however many materials share it.
#[derive(Default)]
struct FileCache {
    read: HashMap<String, Result<TextureFiles, TextureFilesError>>,
}

impl FileCache {
    fn located(&mut self, library: &ModelLibrary, path: String) -> LocatedTexture {
        let files = self.read.entry(path.clone()).or_insert_with(|| texture_files(library.vfs(), &path)).clone();

        LocatedTexture { path, files }
    }
}

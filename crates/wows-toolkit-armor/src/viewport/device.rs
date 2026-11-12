//! The viewport pipeline on a device a host renders with.
//!
//! A host owns the device and says which generation of it is current; a new
//! generation means every resource made on an earlier one is gone. A
//! [`ViewportDevice`] holds the pipeline and releases the mesh buffers of the
//! viewports drawing on it when it is dropped, so a host drops it when its
//! generation is retired, clears each [`Viewport3D`] and uploads again on the
//! next one.

use std::cell::RefCell;

use crate::viewport::renderer::GpuPipeline;
use crate::viewport::renderer::MeshRelease;
use crate::viewport::renderer::SceneAttachments;
use crate::viewport::renderer::Viewport3D;
use crate::viewport::types::TargetSize;

/// A host's device and queue, generation `G`, with the pipeline the
/// viewports draw with on it.
pub struct ViewportDevice<G> {
    device: wgpu::Device,
    queue: wgpu::Queue,
    generation: G,
    pipeline: GpuPipeline,
    /// The mesh buffers of the viewports drawing on this device.
    meshes: RefCell<Vec<MeshRelease>>,
}

impl<G> ViewportDevice<G>
where
    G: Copy + Eq,
{
    /// `pipeline` must be compiled on `device`. [`GpuPipeline::new`] is slow
    /// enough that a host may run it off its UI thread and pass it in here.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, generation: G, pipeline: GpuPipeline) -> Self {
        Self { device, queue, generation, pipeline, meshes: RefCell::default() }
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn pipeline(&self) -> &GpuPipeline {
        &self.pipeline
    }

    pub fn generation(&self) -> G {
        self.generation
    }

    /// Whether resources made here are usable on the host's `generation`.
    pub fn is_generation(&self, generation: G) -> bool {
        self.generation == generation
    }

    /// Releases `viewport`'s mesh buffers along with this device. Call once
    /// per viewport before it uploads here.
    pub fn track(&self, viewport: &Viewport3D) {
        self.meshes.borrow_mut().push(viewport.mesh_release());
    }

    /// Multisampled color and depth attachments of `size` for this pipeline.
    pub fn attachments(&self, size: TargetSize) -> SceneAttachments {
        SceneAttachments::new(&self.device, &self.pipeline, size)
    }

    /// Draws `viewport` into `resolve_target`, a single-sample view in the
    /// pipeline's color format and `attachments`' size, and submits the work.
    pub fn render(&self, viewport: &Viewport3D, attachments: &SceneAttachments, resolve_target: &wgpu::TextureView) {
        viewport.render(&self.device, &self.queue, &self.pipeline, attachments, resolve_target);
    }
}

impl<G> Drop for ViewportDevice<G> {
    fn drop(&mut self) {
        for meshes in self.meshes.get_mut() {
            meshes.release();
        }
    }
}

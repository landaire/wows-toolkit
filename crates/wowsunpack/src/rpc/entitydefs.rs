//use crate::script_type::TypeAliases;
use crate::{
    data::DataFileLoader,
    rpc::typedefs::{ArgType, TypeAliases, parse_aliases, parse_type},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Flags {
    AllClients,
    CellPublicAndOwn,
    OwnClient,
    BaseAndClient,
    Base,
    CellPrivate,
    CellPublic,
    OtherClients,
}

impl Flags {
    fn from_str(s: &str) -> Self {
        if s == "ALL_CLIENTS" {
            Self::AllClients
        } else if s == "CELL_PUBLIC_AND_OWN" {
            Self::CellPublicAndOwn
        } else if s == "OWN_CLIENT" {
            Self::OwnClient
        } else if s == "BASE_AND_CLIENT" {
            Self::BaseAndClient
        } else if s == "BASE" {
            Self::Base
        } else if s == "CELL_PRIVATE" {
            Self::CellPrivate
        } else if s == "CELL_PUBLIC" {
            Self::CellPublic
        } else {
            panic!("Unrecognized flag {s}!");
        }
    }
}

#[derive(Clone, Debug)]
pub struct Property {
    pub name: String,
    pub prop_type: ArgType,
    pub flags: Flags,
}

#[derive(Debug)]
pub struct Method {
    pub name: String,
    variable_length_header_size: usize,
    pub args: Vec<ArgType>,
}

impl Method {
    fn sort_size(&self) -> usize {
        let size = self.args.iter().map(|arg| arg.sort_size()).sum::<usize>();
        if size >= 0xffff { 0xffff + self.variable_length_header_size } else { size + self.variable_length_header_size }
    }
}

struct DefFile {
    base_methods: Vec<Method>,
    cell_methods: Vec<Method>,
    client_methods: Vec<Method>,
    properties: Vec<Property>,
    implements: Vec<String>,
}

pub struct EntitySpec {
    pub name: String,
    pub base_methods: Vec<Method>,
    pub cell_methods: Vec<Method>,
    pub client_methods: Vec<Method>,
    pub properties: Vec<Property>,
    pub internal_properties: Vec<Property>,
    pub base_properties: Vec<Property>,
}

fn child_by_name<'a, 'b>(node: &roxmltree::Node<'a, 'b>, name: &str) -> Option<roxmltree::Node<'a, 'b>> {
    node.children().find(|&child| child.tag_name().name() == name)
}

fn parse_implements(ilist: &roxmltree::Node) -> Vec<String> {
    let mut implements = vec![];
    for implement in ilist.children() {
        if !implement.is_element() {
            continue;
        }

        implements.push(implement.text().unwrap().to_string());
    }
    implements
}

fn parse_properties(plist: &roxmltree::Node, aliases: &TypeAliases) -> Vec<Property> {
    let mut properties = vec![];
    for property in plist.children() {
        if !property.is_element() {
            continue;
        }

        properties.push(Property {
            name: property.tag_name().name().to_string(),
            prop_type: parse_type(&child_by_name(&property, "Type").unwrap(), aliases),
            flags: Flags::from_str(child_by_name(&property, "Flags").unwrap().text().unwrap().trim()),
        });
    }
    properties
}

fn parse_method(method: &roxmltree::Node, aliases: &TypeAliases) -> Method {
    let mut args = vec![];
    for child in method.children() {
        if child.tag_name().name() == "Arg" {
            //args.push(child.text().unwrap().to_string());
            args.push(parse_type(&child, aliases));
        }
        if child.tag_name().name() == "Args" {
            //println!("{:#?}", child);
            for child in child.children() {
                //println!("{:#?}", child);
                //if child.tag_name().name() == "Arg" {
                // The tag name is the arg name
                if child.is_element() {
                    args.push(parse_type(&child, aliases));
                }
                //}
            }
            //println!("{:#?}", args);
            //panic!();
        }
    }
    let variable_length_header_size = match child_by_name(method, "VariableLengthHeaderSize") {
        Some(x) => {
            //println!("{}: {:#?}", method.tag_name().name(), x.first_child());
            x.first_child().unwrap().text().unwrap().trim().parse::<usize>().unwrap_or(1)
        }
        None => 1,
    };
    Method { name: method.tag_name().name().to_string(), variable_length_header_size, args }
}

fn parse_method_list(mlist: &roxmltree::Node, aliases: &TypeAliases) -> Vec<Method> {
    let mut methods = vec![];
    for method in mlist.children() {
        if !method.is_element() {
            continue;
        }
        methods.push(parse_method(&method, aliases));
    }
    methods
}

fn parse_def(def: &[u8], aliases: &TypeAliases) -> DefFile {
    let def = std::str::from_utf8(def).unwrap();
    //let def = std::fs::read_to_string(&file).unwrap();
    let doc = roxmltree::Document::parse(def).unwrap();
    let root = doc.root();
    let root = child_by_name(&root, "root").unwrap();
    //println!("{:?}", doc);

    // Parse out Implements, Properties, and ClientMethods
    let mut def = DefFile {
        base_methods: child_by_name(&root, "BaseMethods").map(|n| parse_method_list(&n, aliases)).unwrap_or_default(),
        cell_methods: child_by_name(&root, "CellMethods").map(|n| parse_method_list(&n, aliases)).unwrap_or_default(),
        client_methods: child_by_name(&root, "ClientMethods")
            .map(|n| parse_method_list(&n, aliases))
            .unwrap_or_default(),
        properties: child_by_name(&root, "Properties").map(|n| parse_properties(&n, aliases)).unwrap_or_default(),
        implements: child_by_name(&root, "Implements").map(|n| parse_implements(&n)).unwrap_or_default(),
    };
    def.client_methods.sort_by_key(|method| method.sort_size());
    def

    /*let mut implements = vec![];
    for child in root.first_child().unwrap().children() {
        //println!("{}", child.tag_name().name());
        if child.tag_name().name() == "Implements" {
            implements = parse_implements(&child);
            println!("Implements: {:#?}", implements);
        } else if child.tag_name().name() == "Properties" {
            let properties = parse_properties(&child);
            println!("{:#?}", properties);
        } else if child.tag_name().name() == "ClientMethods" {
            let mut client_methods = vec![];
            for method in child.children() {
                if !method.is_element() {
                    continue;
                }
                client_methods.push(parse_method(&method));
            }
            println!("{:#?}", client_methods);
        } else if child.tag_name().name() == "CellMethods" {
            let mut cell_methods = vec![];
            for method in child.children() {
                if !method.is_element() {
                    continue;
                }
                cell_methods.push(parse_method(&method));
            }
            println!("{:#?}", cell_methods);
        } else if child.tag_name().name() == "BaseMethods" {
            let mut base_methods = vec![];
            for method in child.children() {
                if !method.is_element() {
                    continue;
                }
                base_methods.push(parse_method(&method));
            }
            println!("{:#?}", base_methods);
        }
    }
    DefFile { implements }*/
}

pub fn parse_scripts(gamedata: &impl DataFileLoader) -> Result<Vec<EntitySpec>, crate::error::GameDataError> {
    /*let alias_path = gamedata.lookup("scripts/entity_defs/alias.xml");

    let aliases = parse_aliases(&alias_path);*/
    let aliases = parse_aliases(&gamedata.get("scripts/entity_defs/alias.xml")?);

    //let entities_xml_path = gamedata.lookup("scripts/entities.xml");
    //let entities_xml = std::fs::read_to_string(&entities_xml_path).unwrap();
    let entities_xml = gamedata.get("scripts/entities.xml")?;
    let doc = roxmltree::Document::parse(std::str::from_utf8(&entities_xml).unwrap()).unwrap();
    let root = doc.root();
    let mut entities = vec![];
    for child in child_by_name(&child_by_name(&root, "root").unwrap(), "ClientServerEntities").unwrap().children() {
        if !child.is_element() {
            continue;
        }

        let def = gamedata.get(&format!("scripts/entity_defs/{}.def", child.tag_name().name()))?;
        let mut def = parse_def(&def, &aliases);
        let inherits = def
            .implements
            .iter()
            .map(|parent| {
                let parent = gamedata.get(&format!("scripts/entity_defs/interfaces/{parent}.def")).unwrap();
                parse_def(&parent, &aliases)
            })
            .flat_map(|mut parent| {
                // Sometimes, our parents have parents of our own. For now we only support
                // a single level of indirection.
                let mut result: Vec<_> = parent
                    .implements
                    .iter()
                    .map(|parent| {
                        let parent = parent.trim();
                        let parent = gamedata.get(&format!("scripts/entity_defs/interfaces/{parent}.def")).unwrap();
                        //println!("Parsing parent {}...", parent);
                        parse_def(&parent, &aliases)
                    })
                    .collect();
                parent.implements = vec![];
                result.push(parent);
                result
            })
            .fold(
                DefFile {
                    base_methods: vec![],
                    cell_methods: vec![],
                    client_methods: vec![],
                    properties: vec![],
                    implements: vec![],
                },
                |mut a, mut b| {
                    a.base_methods.append(&mut b.base_methods);
                    a.cell_methods.append(&mut b.cell_methods);
                    a.client_methods.append(&mut b.client_methods);
                    a.properties.append(&mut b.properties);
                    assert!(a.implements.is_empty());
                    assert!(b.implements.is_empty());
                    a
                },
            );
        /*println!(
            "{} has {} properties + {} inherited properties",
            child.tag_name().name(),
            def.properties.len(),
            inherits.properties.len()
        );*/

        let mut base_methods = inherits.base_methods;
        base_methods.append(&mut def.base_methods);

        let mut cell_methods = inherits.cell_methods;
        cell_methods.append(&mut def.cell_methods);

        let mut client_methods = inherits.client_methods;
        client_methods.append(&mut def.client_methods);

        client_methods.sort_by_key(|method| method.sort_size());

        let mut properties = inherits.properties;
        properties.append(&mut def.properties);

        /*
            EntityFlags.ALL_CLIENTS |
            # not used for some reason
            # EntityFlags.BASE_AND_CLIENT |
            EntityFlags.OTHER_CLIENTS |
            EntityFlags.OWN_CLIENT |
            EntityFlags.CELL_PUBLIC_AND_OWN |
            EntityFlags.ALL_CLIENTS
        */
        let internal_properties: Vec<_> = properties
            .iter()
            .filter(|property| {
                property.flags == Flags::AllClients
                    || property.flags == Flags::OtherClients
                    || property.flags == Flags::OwnClient
                    || property.flags == Flags::CellPublicAndOwn
            })
            .map(|property| (*property).clone())
            .collect();

        //internal_properties.sort_by_key(|prop| prop.prop_type.sort_size());

        let base_properties: Vec<_> = properties
            .iter()
            .filter(|property| property.flags == Flags::BaseAndClient)
            .map(|property| (*property).clone())
            .collect();

        //base_properties.sort_by_key(|prop| prop.prop_type.sort_size());

        properties = properties
            .iter()
            .filter(|property| {
                /*
                            EntityFlags.ALL_CLIENTS |
                            EntityFlags.BASE_AND_CLIENT |
                            EntityFlags.OTHER_CLIENTS |
                            EntityFlags.OWN_CLIENT |
                            EntityFlags.CELL_PUBLIC_AND_OWN |
                */
                property.flags == Flags::AllClients
                    || property.flags == Flags::BaseAndClient
                    || property.flags == Flags::OtherClients
                    || property.flags == Flags::OwnClient
                    || property.flags == Flags::CellPublicAndOwn
            })
            .map(|property| (*property).clone())
            .collect();

        properties.sort_by_key(|prop| prop.prop_type.sort_size());

        entities.push(EntitySpec {
            name: child.tag_name().name().to_string(),
            base_methods,
            cell_methods,
            client_methods,
            properties,
            internal_properties,
            base_properties,
        });
    }

    /*for EntitySpec in entities.iter() {
        println!(
            "{} has {} properties and {}/{}/{} base/cell/client methods",
            EntitySpec.name,
            EntitySpec.properties.len(),
            EntitySpec.base_methods.len(),
            EntitySpec.cell_methods.len(),
            EntitySpec.client_methods.len()
        );

        if EntitySpec.name == "Vehicle" || EntitySpec.name == "Avatar" {
            for (i, property) in EntitySpec.properties.iter().enumerate() {
                println!(
                    " - {}: {} size={} type={:?}",
                    i,
                    property.name,
                    property.prop_type.sort_size(),
                    property.prop_type,
                );
            }
            println!("Methods:");
            for (i, method) in EntitySpec.client_methods.iter().enumerate() {
                println!(
                    " - {}: {}: size {} args: {:?}",
                    i,
                    method.name,
                    method.sort_size(),
                    method.args
                );
            }
        }
    }*/
    Ok(entities)
}

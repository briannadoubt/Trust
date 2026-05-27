rustricted_attrs::strict!{}

use super::file_tree::{File, FileTree, FileType, TypeSpecficData};
use serde::Serialize;

#[derive(Serialize)]
struct SerializableTreeFile<'a> {
    pub name: &'a String,
    pub path: &'a String,
}

#[derive(Serialize)]
struct SerializableTreeLink<'a> {
    pub name: &'a String,
    pub path: &'a String,
    pub link: String,
}

#[derive(Serialize)]
struct SerializableTreeDirectory<'a> {
    pub name: &'a String,
    pub path: &'a String,
    pub contents: Vec<SerializableTreeNode<'a>>,
}

#[derive(Serialize)]
#[serde(tag = "type")]
#[serde(rename_all = "lowercase")]
enum SerializableTreeNode<'a> {
    File(SerializableTreeFile<'a>),
    Link(SerializableTreeLink<'a>),
    Directory(Box<SerializableTreeDirectory<'a>>),
}

impl SerializableTreeNode<'_> {
    // Renamed from `new` to avoid a per-file registry collision with
    // `FileTree::new(root_path, children)` (arity 2) below: the rustricted
    // lower pass keys callees by short name only, so both methods register
    // as `new` and R3001/R0042 fire on either call form. See RT-29 followups.
    pub fn from_tree(tree: &FileTree) -> SerializableTreeNode {
        let root = tree.storage.get(tree.root_id).expect("FileTree invariant: root_id always present in storage");
        SerializableTreeNode::build(tree: tree, file: root.as_ref())
    }

    // Renamed from `from` to avoid colliding with the `From` trait method.
    fn build<'a>(tree: &'a FileTree, file: &'a File) -> SerializableTreeNode<'a> {
        match &file.data {
            TypeSpecficData::File => SerializableTreeNode::File(SerializableTreeFile {
                name: &file.display_name,
                path: &file.path,
            }),
            TypeSpecficData::Directory(map) => {
                SerializableTreeNode::Directory(Box::new(SerializableTreeDirectory {
                    name: &file.display_name,
                    path: &file.path,
                    contents: map
                        .values()
                        .map(|id| {
                            // reason: id values come from this same FileTree's child map; absence indicates a
                            // logic bug in tree construction (panic is appropriate)
                            let child = tree.storage.get(*id).expect("FileTree invariant: child id always present in storage");
                            SerializableTreeNode::build(tree: tree, file: child.as_ref())
                        })
                        .collect(),
                }))
            }
            TypeSpecficData::Link(link) => SerializableTreeNode::Link(SerializableTreeLink {
                name: &file.display_name,
                path: &file.path,
                link: link.clone(),
            }),
        }
    }
}

pub fn format_paths(root_path: &str, children: Vec<(String, FileType)>) -> String {
    match FileTree::new(root_path, children) {
        Some(tree) => {
            let node = SerializableTreeNode::from_tree(&tree);
            serde_json::to_string_pretty(&node).unwrap_or_else(|_| "{}".to_string())
        }
        None => "{}".to_string(),
    }
}

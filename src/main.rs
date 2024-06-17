use std::{
    collections::HashMap,
    env::args,
    fmt::Display,
    fs::{create_dir_all, read_dir, read_to_string, write},
    io::ErrorKind,
    path::{Path, PathBuf},
    str::FromStr,
};

use chrono::{DateTime, FixedOffset, Local};
use derive_more::{Deref, DerefMut};
use petgraph::{
    graph::{DiGraph, NodeIndex},
    visit::EdgeRef,
    Direction::Incoming,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
struct Site {
    notes: DiGraph<Note, Connection>,
    note_indexes: HashMap<NoteId, NodeIndex>,
    top_levels: Vec<NoteId>,
}

type NoteId = u32;

#[derive(Serialize, Deserialize, Clone)]
struct Note {
    id: NoteId,
    alternative: Option<String>,
    create_at: DateTime<FixedOffset>,
    update_at: Vec<DateTime<FixedOffset>>,
    title: Option<String>,
    content: NoteContent,
}

#[derive(Serialize, Deserialize, Clone)]
enum NoteContent {
    PlainText(Vec<String>),
    Asset(PathBuf),
}

#[derive(Serialize, Deserialize)]
enum Connection {
    Own,
    Cause,
}

#[derive(Deref, DerefMut)]
struct ConnectedNote {
    #[deref]
    #[deref_mut]
    inner: Note,
    parent_id: Option<NoteId>,
    previous_ids: Vec<NoteId>,
}

impl FromStr for ConnectedNote {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut id = None;
        let mut alternative = None;
        let mut create_at = None;
        let mut update_at = Vec::new();
        let mut title = None;
        let mut parent = None;
        let mut previous = Vec::new();

        let mut lines = s.lines();
        while let Some(line) = lines.next() {
            let line = line.trim();
            if line.is_empty() {
                break;
            }
            let value = lines
                .next()
                .ok_or(anyhow::format_err!("missing value for `{line}` record"))?
                .trim();
            match line {
                "id" => id = Some(value.parse()?),
                "alternative" => alternative = Some(value.into()),
                "create" => create_at = Some(DateTime::parse_from_rfc3339(value)?),
                "update" => update_at.push(DateTime::parse_from_rfc3339(value)?),
                "title" => title = Some(value.into()),
                "parent" => parent = Some(value.parse()?),
                "previous" => previous.push(value.parse()?),
                _ => anyhow::bail!("unrecognized record `{line}`"),
            }
        }
        let content = NoteContent::PlainText(
            lines
                .filter_map(|line| {
                    let line = line.trim();
                    if line.is_empty() {
                        None
                    } else {
                        Some(line.into())
                    }
                })
                .collect(),
        );
        let note = Note {
            id: id.ok_or(anyhow::format_err!("missing `id` record"))?,
            alternative,
            create_at: create_at.ok_or(anyhow::format_err!("missing `create` record"))?,
            update_at,
            title,
            content,
        };
        Ok(Self {
            inner: note,
            parent_id: parent,
            previous_ids: previous,
        })
    }
}

impl Display for ConnectedNote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "id")?;
        writeln!(f, "{}", self.id)?;
        if let Some(alternative) = &self.alternative {
            writeln!(f, "alternative")?;
            writeln!(f, "{alternative}")?
        }
        writeln!(f, "create")?;
        writeln!(f, "{}", self.create_at.to_rfc3339())?;
        for date_time in &self.update_at {
            writeln!(f, "update")?;
            writeln!(f, "{}", date_time.to_rfc3339())?
        }
        if let Some(parent_id) = &self.parent_id {
            writeln!(f, "parent")?;
            writeln!(f, "{parent_id}")?
        }
        for note_id in &self.previous_ids {
            writeln!(f, "previous")?;
            writeln!(f, "{note_id}")?
        }
        if let Some(title) = &self.title {
            writeln!(f, "title")?;
            writeln!(f, "{title}")?
        }
        match &self.content {
            NoteContent::PlainText(paragraphs) => {
                for paragraph in paragraphs {
                    writeln!(f)?;
                    writeln!(f, "{paragraph}")?
                }
            }
            NoteContent::Asset(path) => {
                writeln!(f, "type")?;
                writeln!(f, "asset")?;
                writeln!(f, "path")?;
                writeln!(f, "{}", path.display())?
            }
        }
        Ok(())
    }
}

impl Site {
    fn new() -> Self {
        Self::default()
    }

    fn find(&self, key: &str) -> anyhow::Result<&Note> {
        let parent_note = if let Some(id) = key.strip_prefix('@') {
            let id = id.parse::<NoteId>()?;
            self.notes.node_weights().find(|note| note.id == id)
        } else {
            self.notes
                .node_weights()
                .find(|note| note.alternative.as_deref() == Some(key))
        }
        .ok_or(anyhow::format_err!("note `{key}` not found"))?;
        Ok(parent_note)
    }

    fn make_connected(&self, id: NoteId) -> ConnectedNote {
        let index = self.note_indexes[&id];
        let parent_id = self.notes.edges_directed(index, Incoming).find_map(|edge| {
            if matches!(edge.weight(), Connection::Own) {
                Some(self.notes[edge.source()].id)
            } else {
                None
            }
        });
        let previous_ids = self
            .notes
            .edges_directed(index, Incoming)
            .filter_map(|edge| {
                if matches!(edge.weight(), Connection::Cause) {
                    Some(self.notes[edge.source()].id)
                } else {
                    None
                }
            })
            .collect();
        ConnectedNote {
            inner: self.notes[index].clone(),
            parent_id,
            previous_ids,
        }
    }
}

const NOTES_DIR: &str = "notes";

fn index() -> anyhow::Result<Site> {
    let mut site = Site::new();
    let read_dir = match read_dir(Path::new(NOTES_DIR)) {
        Ok(read_dir) => read_dir,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(site),
        err => err?,
    };
    let mut note_parents = HashMap::new();
    let mut note_previous = HashMap::new();
    for entry in read_dir {
        let path = entry?.path();
        let note = read_to_string(path)?.parse::<ConnectedNote>()?;
        let note_id = note.id;
        let index = site.notes.add_node(note.inner);
        site.note_indexes.insert(note_id, index);
        if let Some(parent_id) = note.parent_id {
            note_parents.insert(note_id, parent_id);
        }
        if !note.previous_ids.is_empty() {
            note_previous.insert(note_id, note.previous_ids);
        }
    }
    for (note_id, parent_id) in note_parents {
        site.notes.add_edge(
            site.note_indexes[&parent_id],
            site.note_indexes[&note_id],
            Connection::Own,
        );
    }
    for (note_id, previous_ids) in note_previous {
        for id in previous_ids {
            site.notes.add_edge(
                site.note_indexes[&id],
                site.note_indexes[&note_id],
                Connection::Cause,
            );
        }
    }
    Ok(site)
}

fn new_note(site: &Site, belongs_to: Option<&str>) -> anyhow::Result<()> {
    let id = site
        .notes
        .node_weights()
        .map(|note| note.id)
        .max()
        .unwrap_or_default()
        + 1;
    let note = Note {
        id,
        alternative: None,
        create_at: Local::now().into(),
        update_at: Default::default(),
        title: None,
        content: NoteContent::PlainText(Default::default()),
    };
    let mut note = ConnectedNote {
        inner: note,
        parent_id: None,
        previous_ids: Default::default(),
    };
    if let Some(belongs_to) = belongs_to {
        note.parent_id = Some(site.find(belongs_to)?.id)
    }

    let path = Path::new(NOTES_DIR);
    create_dir_all(path)?;
    let path = path.join(format!("{id}.txt"));
    write(&path, note.to_string())?;
    println!("{}", path.display());
    Ok(())
}

fn update_note(site: &Site, key: &str) -> anyhow::Result<()> {
    let id = site.find(key)?.id;
    let mut note = site.make_connected(id);
    note.update_at.push(Local::now().into());
    let path = Path::new(NOTES_DIR);
    create_dir_all(path)?;
    let path = path.join(format!("{id}.txt"));
    write(&path, note.to_string())?;
    println!("{}", path.display());
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let site = index()?;
    println!(
        "Built site index: {} notes, {} connections",
        site.notes.node_count(),
        site.notes.edge_count()
    );
    let Some(command) = args().nth(1) else {
        return Ok(());
    };
    let key = args().nth(2);
    match &*command {
        "new" => new_note(&site, key.as_deref()),
        "update" => update_note(
            &site,
            &key.ok_or(anyhow::format_err!("missing note argument"))?,
        ),
        _ => anyhow::bail!("unrecognized command `{command}`"),
    }
}
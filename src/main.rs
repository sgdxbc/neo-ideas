use std::{
    collections::HashMap,
    env::args,
    fmt::{Display, Write},
    fs::{copy, create_dir_all, read_dir, read_to_string, write},
    hash::{BuildHasher, RandomState},
    io::ErrorKind,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::Context as _;
use chrono::{DateTime, FixedOffset, Local, Locale::zh_CN, Utc};
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
    #[serde(skip)]
    random_state: RandomState,
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
    Image(PathBuf),
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
    top_level: bool,
}

impl FromStr for ConnectedNote {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut id = None;
        let mut alternative = None;
        let mut create_at = None;
        let mut update_at = Vec::new();
        let mut title = None;
        let mut parent_id = None;
        let mut previous_ids = Vec::new();
        let mut top_level = false;

        let mut lines = s.lines();
        while let Some(line) = lines.next() {
            let line = line.trim();
            if line.is_empty() {
                break;
            }
            match line {
                "top level" => {
                    top_level = true;
                    continue;
                }
                "image" => {
                    // TODO
                    continue;
                }
                _ => {}
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
                "parent" => parent_id = Some(value.parse()?),
                "previous" => previous_ids.push(value.parse()?),
                _ => anyhow::bail!("unrecognized record `{line}`"),
            }
        }
        // TODO type
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
            parent_id,
            previous_ids,
            top_level,
        })
    }
}

impl Display for ConnectedNote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "id")?;
        if self.top_level {
            writeln!(f, "top level")?
        }
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
            NoteContent::Image(path) => {
                writeln!(f, "image")?;
                writeln!(f)?;
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
            top_level: self.top_levels.contains(&id),
        }
    }

    fn render_single(&self, note: &Note, current: bool, site_url: &str) -> anyhow::Result<String> {
        let background_hue = self.random_state.hash_one(note.id.to_string()) % 360;

        let id = format!(
            r#"<div class="note-id"><small>@{}{}</small></div>"#,
            note.id,
            if let Some(alternative) = &note.alternative {
                format!(" ({alternative})")
            } else {
                Default::default()
            }
        );
        let title = note
            .title
            .as_ref()
            .map(|title| format!("<h1>{title}</h1>"))
            .unwrap_or_default();
        let create_at = note.create_at.format_localized("%c %z", zh_CN);
        let update_at = note
            .update_at
            .iter()
            .max()
            .map(|at| format!(" / {}", at.format_localized("%c %z", zh_CN)))
            .unwrap_or_default();
        let metadata = format!(r#"<p class="metadata sans-serif">{create_at}{update_at}</p>"#);
        let content = match &note.content {
            NoteContent::PlainText(paragraphs) => {
                paragraphs.iter().fold(String::new(), |mut s, paragraph| {
                    let _ = write!(s, "<p>{paragraph}</p>"); // clippy says i can ignore error
                    s
                })
            }
            NoteContent::Image(path) => {
                let target_path = Path::new(RENDER_DIR).join(path);
                if let Some(path) = target_path.parent() {
                    create_dir_all(path)?
                }
                copy(Path::new(NOTES_DIR).join(path), target_path)?;
                format!(r#"<img src="{site_url}/{}">"#, path.display())
            }
        };
        Ok(format!(
            r#"
            <div class="note {}" style="background: hsla({background_hue} 100 99 / 0.8);">
                {id}
                {title}
                {metadata}
                <hr>
                {content}
            </div>
            "#,
            if current { "current" } else { "child" }
        ))
    }

    fn render(&self, note: &Note, site_url: &str) -> anyhow::Result<String> {
        let mut rendered = self.render_single(note, true, site_url)?;
        let mut owned_indexes = self
            .notes
            .edges(self.note_indexes[&note.id])
            .filter_map(|edge| {
                if matches!(edge.weight(), Connection::Own) {
                    Some(edge.target())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        owned_indexes.sort_unstable_by_key(|index| self.notes[*index].create_at);
        for index in owned_indexes {
            let note = &self.notes[index];
            rendered += &format!(
                r#"<a href={site_url}/{} style="color: inherit; text-decoration: inherit;">{}</a>"#,
                if let Some(alternative) = &note.alternative {
                    alternative.into()
                } else {
                    note.id.to_string()
                },
                self.render_single(note, false, site_url)?
            )
        }
        Ok(rendered)
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
        top_level: false,
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

const RENDER_DIR: &str = "target/web";

fn render(site: &Site, site_url: &str) -> anyhow::Result<()> {
    let path = Path::new(RENDER_DIR);
    create_dir_all(path)?;
    for note in site.notes.node_weights() {
        let title = if let Some(title) = &note.title {
            title
        } else {
            &format!("@{}", note.id)
        };
        let rendered = format!(
            include_str!("page.html"),
            site.render(note, site_url)?,
            site_url = site_url,
            title = title,
            style = include_str!("style.css"),
            now = Utc::now(),
            seed = site.random_state.hash_one(0),
        );
        if let Some(alternative) = &note.alternative {
            let path = path.join(alternative);
            create_dir_all(&path).context(path.display().to_string())?;
            write(path.join("index.html"), &rendered)?
        }
        let path = path.join(note.id.to_string());
        create_dir_all(&path).context(path.display().to_string())?;
        write(path.join("index.html"), rendered)?;
    }
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
    let arg2 = args().nth(2);
    match &*command {
        "new" => new_note(&site, arg2.as_deref()),
        "update" => update_note(
            &site,
            &arg2.ok_or(anyhow::format_err!("missing note argument"))?,
        ),
        "render" => render(&site, &arg2.unwrap_or_default()),
        _ => anyhow::bail!("unrecognized command `{command}`"),
    }
}

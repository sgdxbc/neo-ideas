use std::{
    io::ErrorKind,
    sync::{Arc, Mutex},
};

use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
    Router,
};
use chrono::{DateTime, FixedOffset, Local};
use serde::{Deserialize, Serialize};
use tokio::{fs, spawn};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let site = 'site: {
        let site = match fs::read("site.json").await {
            Ok(site) => site,
            Err(err) if err.kind() == ErrorKind::NotFound => break 'site None,
            err => err?,
        };
        Some(serde_json::from_slice::<Site>(&site)?)
    };
    let site = site.unwrap_or_else(|| {
        println!("Start with default `site.json`");
        Site {
            name: "no name".into(),
            items: Default::default(),
            anchors: Default::default(),
        }
    });

    let app = Router::new()
        .route("/", get(home))
        .route("/edit/new", post(create_item))
        .with_state(App {
            site: Arc::new(Mutex::new(site)),
        });
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    let serve_handle = spawn(async move { axum::serve(listener, app).await });
    tokio::select! {
        result = serve_handle => result??,
        result = tokio::signal::ctrl_c() => {
            result?;
            println!()
        }
    }
    Ok(())
}

#[derive(Clone)]
struct App {
    site: Arc<Mutex<Site>>,
}

#[derive(Serialize, Deserialize)]
struct Site {
    name: String,
    items: Vec<Item>,
    anchors: Vec<u32>,
}

#[derive(Serialize, Deserialize)]
struct Item {
    id: u32,
    alternative_path: Option<String>,
    create_at: DateTime<FixedOffset>,
    update_at: Option<DateTime<FixedOffset>>,
    content: String,
    children: Vec<u32>,
}

async fn home(State(app): State<App>) -> impl IntoResponse {
    let site = app.site.lock().unwrap();
    // TODO display anchors
    Html(
        format!("<div><b>{}</b></div>", site.name)
            + r#"<form action="/edit/new" method="post"><input type="submit" value="New"></form>"#,
    )
}

#[derive(Deserialize)]
struct CreateItemQuery {
    parent_id: Option<u32>,
}

async fn create_item(
    State(app): State<App>,
    Query(query): Query<CreateItemQuery>,
) -> impl IntoResponse {
    let mut site = app.site.lock().unwrap();
    let id = site.items.last().map(|item| item.id).unwrap_or_default() + 1;
    let item = Item {
        id,
        alternative_path: None,
        create_at: Local::now().fixed_offset(),
        update_at: None,
        content: Default::default(),
        children: Default::default(),
    };
    site.items.push(item);
    if let Some(parent_id) = query.parent_id {
        site.items
            .iter_mut()
            .find(|item| item.id == parent_id)
            .unwrap()
            .children
            .push(id);
        Redirect::to(&format!("/{parent_id}"))
    } else {
        Redirect::to(&format!("/edit/{id}"))
    }
}

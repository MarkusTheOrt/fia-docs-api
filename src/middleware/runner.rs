use super::{
    magick::{clear_tmp_dir, run_magick},
    parser::{HTMLParser, ParserEvent},
};
use aws_sign_v4::AwsSign;
use chrono::{DateTime, Datelike, Utc};
use f1_bot_types::{Document, DocumentStatus, Event, EventStatus, Series};
use html5ever::{
    tendril::{ByteTendril, ReadExt, fmt::Slice},
    tokenizer::{BufferQueue, Tokenizer, TokenizerOpts},
};
use libsql::{Connection, params};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use std::{
    cell::RefCell,
    fs::File,
    path::PathBuf,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use std::{
    io::{Read, Write},
    time::UNIX_EPOCH,
};
use tracing::info;

const F1_DOCS_URL: &str = "https://www.fia.com/documents/championships/fia-formula-one-world-championship-14/season/season-2025-2071";
const F2_DOCS_URL: &str =
    "https://www.fia.com/documents/season/season-2025-2071/championships/formula-2-championship-44";
const F3_DOCS_URL: &str = "https://www.fia.com/documents/season/season-2025-2071/championships/fia-formula-3-championship-1012";

struct LocalCache {
    pub documents: Vec<Document>,
    pub events: Vec<Event>,
    pub last_populated: DateTime<Utc>,
}

impl Default for LocalCache {
    fn default() -> Self {
        Self {
            events: vec![],
            documents: vec![],
            last_populated: DateTime::from(UNIX_EPOCH),
        }
    }
}

async fn populate_cache(
    db_conn: &Connection,
    cache: &mut LocalCache,
    year: i32,
) -> crate::error::Result {
    cache.events.clear();
    let mut events = db_conn
        .query("SELECT * FROM events WHERE year = ?", params![year])
        .await?;
    while let Ok(Some(event)) = events.next().await {
        cache.events.push(libsql::de::from_row(&event)?);
    }

    let delta = Utc::now() - cache.last_populated;
    // lets revalidate the cache once a day.
    if delta.num_days() < 1 {
        return Ok(());
    }
    let mut docs = db_conn
        .query(
            "SELECT * FROM documents WHERE strftime('%Y', created_at) = ?",
            params![year.to_string()],
        )
        .await?;
    cache.documents.clear();
    while let Ok(Some(doc)) = docs.next().await {
        cache.documents.push(libsql::de::from_row(&doc)?);
    }

    cache.last_populated = Utc::now();
    Ok(())
}

pub async fn runner(db_conn: &Connection, should_stop: Arc<AtomicBool>) -> crate::error::Result {
    let mut local_cache = LocalCache::default();

    tokio::task::yield_now().await;
    let start = Utc::now();
    let year = start.year();
    populate_cache(db_conn, &mut local_cache, year).await?;

    for i in Series::F1.i8()..=Series::F3.i8() {
        let series = Series::from(i);
        let docs_url = match series {
            Series::F1 => F1_DOCS_URL,
            Series::F2 => F2_DOCS_URL,
            Series::F3 => F3_DOCS_URL,
            _ => panic!("F1A Not Supported"),
        };
        runner_internal(
            db_conn,
            year,
            docs_url,
            series,
            &mut local_cache,
            should_stop.clone(),
        )
        .await?;
    }

    Ok(())
}

async fn create_new_event(
    db_conn: &Connection,
    series: Series,
    year: i32,
    event: &ParserEvent,
) -> crate::error::Result<Event> {
    info!("Running 1");
    let event_title = event.title.as_ref().cloned().unwrap();
    db_conn
        .execute(
            "INSERT INTO events (title, year, series, status) VALUES (?, ?, ?, ?)",
            params![event_title.clone(), year, series, EventStatus::NotAllowed],
        )
        .await?;
    info!("inserted event \"{event_title}\"");
    Ok(Event {
        id: db_conn.last_insert_rowid() as u64,
        title: event_title,
        year: year as u16,
        series,
        status: f1_bot_types::EventStatus::NotAllowed,
        created_at: Utc::now(),
    })
}

async fn insert_document(
    db_conn: &Connection,
    event_id: i64,
    title: String,
    url: &str,
    mirror: &str,
) -> crate::error::Result<i64> {
    db_conn
        .execute(
            "INSERT INTO documents (
    event_id, title, href, mirror, status
    ) VALUES (?, ?, ?, ?, ?)",
            params![event_id, title, url, mirror, DocumentStatus::Initial],
        )
        .await?;
    Ok(db_conn.last_insert_rowid())
}

async fn upload_image(data: Vec<u8>, url: &String) -> crate::error::Result<()> {
    let data_digest = sha256::digest(data.as_slice());
    let now = Utc::now();
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("x-amz-content-sha256", data_digest.parse().unwrap());
    headers.insert("x-amz-acl", "public-read".parse().unwrap());
    headers.insert(
        "X-Amz-Date",
        now.format("%Y%m%dT%H%M%SZ").to_string().parse().unwrap(),
    );
    headers.insert("host", "fia.ort.dev".parse().unwrap());
    let secret = std::env::var("S3_SECRET_KEY").unwrap();
    let access = std::env::var("S3_ACCESS_KEY").unwrap();
    let sign = AwsSign::new(
        "PUT",
        url,
        &now,
        &headers,
        "us-east-1",
        &access,
        &secret,
        "s3",
        Some(&data_digest),
    );
    let signature = sign.sign();
    headers.insert(AUTHORIZATION, signature.parse().unwrap());
    headers.insert(CONTENT_TYPE, "image/jpeg".parse().unwrap());
    let client = reqwest::Client::new();
    client.put(url).headers(headers).body(data).send().await?;
    Ok(())
}

async fn runner_internal(
    db_conn: &Connection,
    year: i32,
    url: &str,
    series: Series,
    cache: &mut LocalCache,
    should_stop: Arc<AtomicBool>,
) -> crate::error::Result {
    let season = get_season(url, year).await?;

    for ev in season.events.into_iter() {
        if should_stop.load(Ordering::Relaxed) {
            break;
        }
        let cache_event = cache.events.iter().find(|f| {
            ev.title.as_ref().is_some_and(|t| *t == f.title)
                && ev.season.is_some_and(|s| s == year)
                && f.series == series
        });
        let real_event = match cache_event {
            Some(db_event) => db_event.to_owned(),
            None => &create_new_event(db_conn, series, year, &ev).await?,
        };

        for (i, mut doc) in ev.documents.into_iter().enumerate() {
            if should_stop.load(Ordering::Relaxed) {
                break;
            }
            let (title, url) = (doc.title.take().unwrap(), doc.url.take().unwrap());
            if cache.documents.iter().any(|f| f.href == url) {
                continue;
            }

            let (file, body) = download_file(&url, &format!("doc_{i}")).await?;
            let mirror = upload_mirror(&title, &real_event.title, year, &body).await?;
            let inserted_doc_id =
                insert_document(db_conn, real_event.id as i64, title, &url, &mirror).await?;

            cache.documents.push(Document {
                id: inserted_doc_id as u64,
                event_id: real_event.id,
                href: url,
                mirror,
                status: f1_bot_types::DocumentStatus::Initial,
                created_at: Utc::now(),
            });

            let files = run_magick(file.to_string_lossy(), &format!("doc_{i}")).await?;

            // run_magick takes some time to complete, hence we yield here!
            tokio::task::yield_now().await;

            for (page_number, path) in files.iter().enumerate() {
                let mut file = File::open(path)?;
                let mut buf = Vec::with_capacity(1024 * 1024 * 10);
                file.read_to_end(&mut buf)?;
                let url = format!(
                    "https://fia.ort.dev/img/{}/{}/{}-{}.jpg",
                    year,
                    urlencoding::encode(&real_event.title),
                    inserted_doc_id,
                    page_number
                );

                upload_image(buf, &url).await?;

                insert_image(db_conn, inserted_doc_id, page_number, url).await?;
            }
            mark_doc_done(inserted_doc_id, db_conn).await?;
        }
        clear_tmp_dir()?;
    }
    Ok(())
}

async fn mark_doc_done(doc_id: i64, db_conn: &Connection) -> crate::error::Result {
    db_conn
        .execute(
            "UPDATE documents SET status = ? WHERE id = ?",
            params![DocumentStatus::ReadyToPost, doc_id],
        )
        .await?;

    Ok(())
}

async fn insert_image(
    db_conn: &Connection,
    doc_id: i64,
    page_number: usize,
    url: String,
) -> crate::error::Result {
    db_conn
        .execute(
            "INSERT INTO images (document_id, url, page_number) VALUES (?, ?, ?)",
            params![doc_id, url, page_number as i64],
        )
        .await?;

    Ok(())
}

async fn upload_mirror(
    title: &str,
    event: &str,
    year: i32,
    content: &Vec<u8>,
) -> crate::error::Result<String> {
    let now = Utc::now();
    let title = urlencoding::encode(title);
    let url = format!("https://fia.ort.dev/mirror/{year}/{event}/{title}.pdf");
    let digest = sha256::digest(content.as_slice());
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("x-amz-content-sha256", digest.parse().unwrap());
    headers.insert("x-amz-acl", "public-read".parse().unwrap());
    headers.insert(
        "X-Amz-Date",
        now.format("%Y%m%dT%H%M%SZ").to_string().parse().unwrap(),
    );
    headers.insert("host", "fia.ort.dev".parse().unwrap());
    let secret = std::env::var("S3_SECRET_KEY").unwrap();
    let access = std::env::var("S3_ACCESS_KEY").unwrap();
    let sign = AwsSign::new(
        "PUT",
        &url,
        &now,
        &headers,
        "us-east-1",
        &access,
        &secret,
        "s3",
        Some(&digest),
    );
    let signature = sign.sign();
    headers.insert(AUTHORIZATION, signature.parse().unwrap());
    headers.insert(CONTENT_TYPE, "application/pdf".parse().unwrap());

    let client = reqwest::Client::new();
    let t = client
        .put(url)
        .headers(headers)
        .body(content.to_owned())
        .send()
        .await?;
    let url = t.url().to_string();
    t.error_for_status()?;
    Ok(url)
}

async fn download_file(url: &str, name: &str) -> crate::error::Result<(PathBuf, Vec<u8>)> {
    let request = reqwest::get(url).await?;
    let mut file = File::create(format!("./tmp/{name}.pdf"))?;
    let body = request.bytes().await?;
    file.set_len(body.len() as u64)?;
    file.write_all(&body)?;
    let path = PathBuf::from_str(&format!("./tmp/{name}.pdf"))?;
    // ensure we're actually pointing to a legit file.
    path.try_exists()?;
    Ok((path, body.to_vec()))
}

async fn get_season(url: &str, year: i32) -> crate::error::Result<super::parser::Season> {
    let request = reqwest::get(url).await?;
    let bytes = request.bytes().await?;
    let mut tendril = ByteTendril::new();
    bytes.as_bytes().read_to_tendril(&mut tendril)?;
    let input = BufferQueue::default();
    input.push_back(tendril.try_reinterpret().unwrap());
    let parser_season = RefCell::new(super::parser::Season {
        year,
        events: vec![],
    });
    {
        let sink = HTMLParser::new(&parser_season);
        let opts = TokenizerOpts::default();
        let tok = Tokenizer::new(sink, opts);
        let _ = tok.feed(&input);
        tok.end();
    }
    Ok(parser_season.into_inner())
}

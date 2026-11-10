//! Playlists through the librespot session, without the Web API quota: the library list from rootlist (folders, permissions, and
//! headers — from rootlist decorations), playlist edits, creation, following, visibility, and cover art.
//!
//! Verified on a live account (spclient, the body is the playlist4_external protobuf):
//!   POST /playlist/v2/playlist            OpList { UPDATE_LIST_ATTRIBUTES } → CreateListReply { uri, revision }
//!   POST /playlist/v2/playlist/{id}/changes             ListChanges { base_revision, Delta { ops } }
//!   POST /playlist/v2/user/{user}/rootlist/changes     the same for rootlist (ADD/REM, visibility —
//!        UPDATE_ITEM_ATTRIBUTES { index, public } without item: with item the service answers 400)
//!   POST https://image-upload.spotify.com/v4/playlist  JPEG → {"uploadToken"}, then
//!   POST /playlist/v2/playlist/{id}/register-image     → picture for UPDATE_LIST_ATTRIBUTES
//! Mov.to_index — "insert before" in the list before the shift (like the Web API's insert_before). The old base
//! is rebased by the server; a REM with a URI that does not match the position — 400 (conflict: re-read and retry).
//! A new rootlist row without attributes becomes public — visibility is written explicitly.
use std::collections::{HashMap, HashSet};

use base64::Engine as _;
use futures::StreamExt as _;
use librespot_core::{Session, error::ErrorKind, spotify_id::SpotifyId};
use librespot_metadata::playlist::attribute::PlaylistAttributes;
use librespot_protocol::playlist4_external::{
    Add, ChangeInfo, CreateListReply, Delta, Item, ItemAttributes, ItemAttributesPartialState, ListAttributeKind,
    ListAttributes, ListAttributesPartialState, ListChanges, ListItems, MetaItem, Mov, Op, OpList,
    RegisterPlaylistImageRequest, RegisterPlaylistImageResponse, Rem, SelectedListContent, SourceInfo,
    UpdateItemAttributes, UpdateListAttributes, op, source_info,
};
use protobuf::{EnumOrUnknown, Message as _, MessageField};
use reqwest::Method;
use reqwest::header::{CONTENT_TYPE, HeaderMap, HeaderValue};
use sf_api::ApiError;
use sf_api::models::{Image, Owner, Playlist, TrackCount};

use super::player::{Rootlist, editable_uris, parse_rootlist};
use super::session_reads::{self, Failure, IMAGE_HOST};

/// Rootlist rows per request.
const ROOTLIST_PAGE: usize = 500;
/// Playlist rows per content read (the service may return fewer — then more requests).
const CONTENTS_PAGE: usize = 1000;
/// The request ceiling for a single content/rootlist read.
const MAX_PAGES: usize = 100;
/// Owners whose names are requested for a list (the rest — by id).
const MAX_OWNER_NAMES: usize = 40;
/// Name/collage requests in flight at the same time.
const PARALLEL: usize = 4;
/// Rows whose album cover art is used to build the collage for a playlist with no cover art of its own.
const MOSAIC_ROWS: u32 = 30;
const MOSAIC_HOST: &str = "https://mosaic.scdn.co";
const IMAGE_UPLOAD: &str = "https://image-upload.spotify.com/v4/playlist";
/// URIs in a single ADD operation.
const ADD_CHUNK: usize = 500;

/// Library playlists: the rootlist tree and its playlists' headers (as /me/playlists would give them).
#[derive(Clone, Debug, Default)]
pub struct Library {
    pub rootlist: Rootlist,
    pub playlists: Vec<Playlist>,
}

/// A playlist edit.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// At the end or before position `at`.
    Add { uris: Vec<String>, at: Option<u32> },
    /// All occurrences of these URIs (like the Web API's DELETE /playlists/{id}/items).
    Remove { uris: Vec<String> },
    /// One row `from` — before the row `insert_before` (indices before the shift); `snapshot` — the revision
    /// the user saw them at (the server rebases it), otherwise the current one.
    Move { from: u32, insert_before: u32, snapshot: Option<String> },
    /// The header; an empty description is erased (the Web API cannot do that).
    Details { name: Option<String>, description: Option<String>, collaborative: Option<bool> },
    /// Cover art: picture from register-image.
    Picture(Vec<u8>),
}

impl Change {
    /// Whether the playlist's current rows are needed to build the operations.
    fn needs_rows(&self) -> bool {
        matches!(self, Self::Remove { .. })
    }

    /// Whether a write can be retried through the Web API if the session's response was lost: an addition and a move
    /// applied twice produce a different list.
    fn repeatable(&self) -> bool {
        !matches!(self, Self::Add { .. } | Self::Move { .. })
    }
}

// --- library list ------------------------------------------------------------------------

/// Library playlists from rootlist: folders, permissions, headers (name, description, cover art, owner,
/// row count, revision, visibility) — without a single Web API request. Owner names — through the profile.
pub async fn library(session: &Session) -> Result<Library, Failure> {
    let mut pages = Vec::new();
    let mut from = 0usize;
    for _ in 0..MAX_PAGES {
        let bytes = session.spclient().get_rootlist(from, Some(ROOTLIST_PAGE)).await?;
        let content = SelectedListContent::parse_from_bytes(&bytes).map_err(|error| Failure::Retry(error.into()))?;
        let Some(contents) = content.contents.into_option() else { break };
        let count = contents.items.len();
        let truncated = contents.truncated();
        pages.push(contents);
        if !truncated || count == 0 {
            break;
        }
        from += count;
    }
    let mut library = library_of(&pages);
    name_owners(session, &mut library.playlists).await;
    Ok(library)
}

/// The tree and headers from rootlist pages.
fn library_of(pages: &[ListItems]) -> Library {
    let mut uris = Vec::new();
    let mut editable = std::collections::BTreeSet::new();
    let mut playlists: Vec<Playlist> = Vec::new();
    let mut seen = HashSet::new();
    for page in pages {
        editable.extend(editable_uris(page));
        for (index, item) in page.items.iter().enumerate() {
            let Some(uri) = item.uri.clone() else { continue };
            let listed = page.meta_items.get(index).and_then(|meta| listed(item, meta));
            if let Some(playlist) = listed
                && seen.insert(playlist.id.clone())
            {
                playlists.push(playlist);
            }
            uris.push(uri);
        }
    }
    Library { rootlist: Rootlist { entries: parse_rootlist(&uris), editable }, playlists }
}

/// A playlist's header from a rootlist row and its decorations; None — not a playlist, or Spotify does not return it.
fn listed(item: &Item, meta: &MetaItem) -> Option<Playlist> {
    let uri = item.uri();
    let id = uri.strip_prefix("spotify:playlist:")?;
    // an unavailable (deleted, hidden) playlist is marked by rootlist with a response code
    if meta.status_code.is_some_and(|code| code >= 400) || meta.attributes.is_none() {
        return None;
    }
    let attributes = PlaylistAttributes::try_from(&*meta.attributes).ok()?;
    let owner = meta.owner_username.clone().filter(|owner| !owner.is_empty());
    Some(Playlist {
        id: id.to_owned(),
        name: attributes.name.clone(),
        uri: uri.to_owned(),
        description: Some(attributes.description.clone()).filter(|text| !text.is_empty()),
        images: session_reads::playlist_images(&attributes),
        owner: Owner {
            uri: owner.as_ref().map(|owner| format!("spotify:user:{owner}")),
            id: owner,
            display_name: None,
        },
        public: item.attributes.public,
        collaborative: attributes.is_collaborative,
        snapshot_id: meta.revision.as_deref().map(session_reads::snapshot),
        items_count: Some(TrackCount { total: u32::try_from(meta.length()).unwrap_or_default() }),
        ..Default::default()
    })
}

/// Owner names (except Spotify's) — one profile request per owner.
async fn name_owners(session: &Session, playlists: &mut [Playlist]) {
    let mut owners: Vec<String> = Vec::new();
    for owner in playlists.iter().filter_map(|playlist| playlist.owner.id.as_deref()) {
        if owner != "spotify" && !owners.iter().any(|known| known == owner) && owners.len() < MAX_OWNER_NAMES {
            owners.push(owner.to_owned());
        }
    }
    let names: HashMap<String, String> = futures::stream::iter(owners)
        .map(|owner| async move {
            let name = session_reads::user_display_name(session, &owner).await;
            name.map(|name| (owner, name))
        })
        .buffer_unordered(PARALLEL)
        .filter_map(std::future::ready)
        .collect()
        .await;
    for playlist in playlists {
        if let Some(name) = playlist.owner.id.as_ref().and_then(|owner| names.get(owner)) {
            playlist.owner.display_name = Some(name.clone());
        }
    }
}

/// A collage from the first rows' album cover art — how the Web API draws a playlist with no cover art of its own.
pub async fn mosaic(session: &Session, id: &str) -> Vec<Image> {
    // the rows are loose: one odd song at the start should not leave a playlist without cover art
    let Ok(tracks) = session_reads::lead_tracks(session, id, MOSAIC_ROWS).await else { return Vec::new() };
    mosaic_from(tracks.iter().filter_map(|track| {
        let album = track.album.as_ref()?;
        album.images.iter().max_by_key(|image| image.width).map(|image| image.url.as_str())
    }))
}

/// A collage from the rows' cover art URLs (in order, repeats do not count) — how the Web API draws a playlist
/// with no cover art of its own. URLs not from i.scdn.co are skipped.
pub fn mosaic_from<'a>(urls: impl IntoIterator<Item = &'a str>) -> Vec<Image> {
    let mut covers: Vec<String> = Vec::new();
    for url in urls {
        if let Some(cover) = url.strip_prefix(IMAGE_HOST)
            && !cover.is_empty()
            && !covers.iter().any(|known| known == cover)
        {
            covers.push(cover.to_owned());
        }
        if covers.len() == 4 {
            break;
        }
    }
    mosaic_images(&covers)
}

/// Four different covers — a collage of three sizes; fewer — the first cover in full.
fn mosaic_images(covers: &[String]) -> Vec<Image> {
    match covers {
        [] => Vec::new(),
        [first, ..] if covers.len() < 4 => {
            vec![Image { url: format!("{IMAGE_HOST}{first}"), width: None, height: None }]
        }
        _ => [640, 300, 60]
            .into_iter()
            .map(|size| Image {
                url: format!("{MOSAIC_HOST}/{size}/{}", covers[..4].concat()),
                width: Some(size),
                height: Some(size),
            })
            .collect(),
    }
}

// --- playlist edits -------------------------------------------------------------------------

/// Apply an edit on the current revision; on a conflict — re-read and retry once.
/// Ok — the snapshot_id after the write (a base64 revision, like the Web API's); None — unknown (re-read).
pub async fn change(session: &Session, id: &str, change: &Change) -> Result<Option<String>, Failure> {
    let playlist = playlist_id(id)?;
    let path = format!("/playlist/v2/playlist/{id}/changes");
    let mut base = match change {
        Change::Move { snapshot: Some(snapshot), .. } => revision_of(snapshot),
        _ => None,
    };
    for attempt in 0..2 {
        let given = base.take();
        let stale = given.is_some();
        let (revision, rows) = if change.needs_rows() {
            contents(session, &playlist).await?
        } else if let Some(given) = given {
            (given, Vec::new())
        } else {
            (head(session, &playlist).await?, Vec::new())
        };
        let ops = ops_for(change, &rows);
        if ops.is_empty() {
            return Ok((!stale).then(|| session_reads::snapshot(&revision)));
        }
        match post(session, &path, &list_changes(revision, ops)).await {
            // on an old base the response is the branch's revision, not the head's: re-read the head
            Ok(_) if stale => return Ok(head(session, &playlist).await.ok().map(|head| session_reads::snapshot(&head))),
            Ok(bytes) => return Ok(Some(session_reads::snapshot(&resulting_revision(&bytes)?))),
            Err(error) if attempt == 0 && conflict(&error) => {
                tracing::debug!("playlist {id} edit didn't apply ({error}); rereading and retrying");
            }
            Err(error) => return Err(written(error, change.repeatable())),
        }
    }
    Err(Failure::Definitive(ApiError::Status { status: 409, message: "The playlist changed — refresh it".into() }))
}

/// The playlist URIs that are already in it (in request order).
pub async fn duplicates(session: &Session, id: &str, uris: &[String]) -> Result<Vec<String>, Failure> {
    if uris.is_empty() {
        return Ok(Vec::new());
    }
    let (_, rows) = contents(session, &playlist_id(id)?).await?;
    let present: HashSet<&str> = rows.iter().map(String::as_str).collect();
    Ok(uris.iter().filter(|uri| present.contains(uri.as_str())).cloned().collect())
}

/// A new playlist: create it and put it first in rootlist with the given visibility.
pub async fn create(session: &Session, name: &str, description: &str, public: bool) -> Result<Playlist, Failure> {
    let mut list = OpList::new();
    list.ops = vec![attributes_op(Some(name), Some(description).filter(|text| !text.is_empty()), None, None)];
    let bytes = post(session, "/playlist/v2/playlist", &list).await.map_err(|error| written(error, true))?;
    let reply = CreateListReply::parse_from_bytes(&bytes).map_err(|error| Failure::Retry(error.into()))?;
    let uri = reply.uri().to_owned();
    let Some(id) = uri.strip_prefix("spotify:playlist:").map(str::to_owned) else {
        return Err(Failure::Retry(anyhow::anyhow!("create answered {uri:?}")));
    };
    // created but not in rootlist — a retry through the Web API would give a second playlist
    follow(session, &uri, Some(public)).await.map_err(|failure| match failure {
        Failure::Retry(error) => Failure::Definitive(ApiError::Network(format!("{error:#}"))),
        definitive => definitive,
    })?;
    let owner = session.username();
    Ok(Playlist {
        id,
        name: name.to_owned(),
        uri,
        description: Some(description.to_owned()).filter(|text| !text.is_empty()),
        owner: Owner { uri: Some(format!("spotify:user:{owner}")), id: Some(owner), display_name: None },
        public: Some(public),
        snapshot_id: reply.revision.as_deref().map(session_reads::snapshot),
        items_count: Some(TrackCount { total: 0 }),
        ..Default::default()
    })
}

/// Cover art: JPEG to image-upload (with the same session token), registration, and writing it into the header.
/// `http` — the application's shared client (with a proxy). Ok — the snapshot after the write.
pub async fn upload_cover(session: &Session, http: &reqwest::Client, id: &str, jpeg: Vec<u8>) -> Result<Option<String>, Failure> {
    let token = session.login5().auth_token().await?;
    let response = http
        .post(IMAGE_UPLOAD)
        .bearer_auth(&token.access_token)
        .header(CONTENT_TYPE, "image/jpeg")
        .body(jpeg)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| Failure::Retry(error.without_url().into()))?;
    let answer: serde_json::Value = response.json().await.map_err(|error| Failure::Retry(error.without_url().into()))?;
    let Some(upload) = answer.get("uploadToken").and_then(serde_json::Value::as_str) else {
        return Err(Failure::Retry(anyhow::anyhow!("image-upload answered without a token")));
    };
    let mut request = RegisterPlaylistImageRequest::new();
    request.upload_token = Some(upload.to_owned());
    let bytes = post(session, &format!("/playlist/v2/playlist/{id}/register-image"), &request)
        .await
        .map_err(|error| written(error, true))?;
    let picture = RegisterPlaylistImageResponse::parse_from_bytes(&bytes)
        .map_err(|error| Failure::Retry(error.into()))?
        .picture
        .filter(|picture| !picture.is_empty())
        .ok_or_else(|| Failure::Retry(anyhow::anyhow!("register-image answered without a picture")))?;
    change(session, id, &Change::Picture(picture)).await
}

// --- rootlist: following and visibility -----------------------------------------------------------

/// Put a playlist first in rootlist ("follow"; your own new one — "add to library").
/// `public` — whether to show it in the profile; None — whatever Spotify decides.
pub async fn follow(session: &Session, uri: &str, public: Option<bool>) -> Result<(), Failure> {
    rootlist_change(session, |_| Some(vec![rootlist_add_op(uri, public)])).await
}

/// Remove a playlist from rootlist (for your own — "delete", as in Spotify's clients).
pub async fn unfollow(session: &Session, uri: &str) -> Result<(), Failure> {
    rootlist_change(session, |rows| rows.iter().any(|row| row == uri).then(|| vec![rootlist_remove_op(uri)])).await
}

/// Profile visibility is an attribute of the rootlist row. The playlist is not in rootlist — Retry (let the Web API decide).
pub async fn set_public(session: &Session, uri: &str, public: bool) -> Result<(), Failure> {
    let (revision, rows) = rootlist_rows(session).await?;
    let Some(index) = rows.iter().position(|row| row == uri) else {
        return Err(Failure::Retry(anyhow::anyhow!("{uri} is not in the rootlist")));
    };
    let path = rootlist_path(session);
    match post(session, &path, &list_changes(revision, vec![public_op(index, public)])).await {
        Ok(_) => Ok(()),
        Err(error) => Err(written(error, true)),
    }
}

/// A rootlist edit based on its current rows; `ops` → None — nothing to do. One retry on a conflict.
async fn rootlist_change(session: &Session, ops: impl Fn(&[String]) -> Option<Vec<Op>>) -> Result<(), Failure> {
    let path = rootlist_path(session);
    for attempt in 0..2 {
        let (revision, rows) = rootlist_rows(session).await?;
        let Some(ops) = ops(&rows) else { return Ok(()) };
        match post(session, &path, &list_changes(revision, ops)).await {
            Ok(_) => return Ok(()),
            Err(error) if attempt == 0 && conflict(&error) => {
                tracing::debug!("rootlist edit didn't apply ({error}); rereading and retrying");
            }
            Err(error) => return Err(written(error, true)),
        }
    }
    Err(Failure::Retry(anyhow::anyhow!("rootlist kept conflicting")))
}

fn rootlist_path(session: &Session) -> String {
    format!("/playlist/v2/user/{}/rootlist/changes", session.username())
}

/// The rootlist revision and the URIs of all its rows (with folder markers).
async fn rootlist_rows(session: &Session) -> Result<(Vec<u8>, Vec<String>), Failure> {
    let mut revision = Vec::new();
    let mut rows = Vec::new();
    let mut from = 0usize;
    for _ in 0..MAX_PAGES {
        let bytes = session.spclient().get_rootlist(from, Some(ROOTLIST_PAGE)).await?;
        let content = SelectedListContent::parse_from_bytes(&bytes).map_err(|error| Failure::Retry(error.into()))?;
        if from == 0 {
            revision = content.revision.clone().unwrap_or_default();
        }
        let Some(contents) = content.contents.into_option() else { break };
        let count = contents.items.len();
        let truncated = contents.truncated();
        rows.extend(contents.items.into_iter().filter_map(|item| item.uri));
        if !truncated || count == 0 {
            break;
        }
        from += count;
    }
    Ok((revision, rows))
}

// --- requests ---------------------------------------------------------------------------------

fn playlist_id(id: &str) -> Result<SpotifyId, Failure> {
    SpotifyId::from_base62(id).map_err(|_| {
        Failure::Definitive(ApiError::Status { status: 400, message: "Invalid playlist link".into() })
    })
}

/// A playlist read refusal: it does not exist or there is no access — final.
fn refused(error: librespot_core::Error) -> Failure {
    match status_of(error.kind) {
        Some(status @ (403 | 404)) => Failure::Definitive(ApiError::Status { status, message: String::new() }),
        _ => error.into(),
    }
}

/// The playlist's current revision (the header only).
async fn head(session: &Session, playlist: &SpotifyId) -> Result<Vec<u8>, Failure> {
    let bytes = session.spclient().get_playlist_range(playlist, 0, 0).await.map_err(refused)?;
    let content = SelectedListContent::parse_from_bytes(&bytes).map_err(|error| Failure::Retry(error.into()))?;
    Ok(content.revision.unwrap_or_default())
}

/// The revision and the URIs of all the playlist's rows, in order.
async fn contents(session: &Session, playlist: &SpotifyId) -> Result<(Vec<u8>, Vec<String>), Failure> {
    let mut revision: Option<Vec<u8>> = None;
    let mut rows = Vec::new();
    for _ in 0..MAX_PAGES {
        let bytes = session.spclient().get_playlist_range(playlist, rows.len(), CONTENTS_PAGE).await.map_err(refused)?;
        let content = SelectedListContent::parse_from_bytes(&bytes).map_err(|error| Failure::Retry(error.into()))?;
        let now = content.revision.clone().unwrap_or_default();
        match &revision {
            Some(first) if *first != now => {
                return Err(Failure::Retry(anyhow::anyhow!("playlist changed while it was read")));
            }
            Some(_) => {}
            None => revision = Some(now),
        }
        let length = usize::try_from(content.length()).unwrap_or_default();
        let items = content.contents.into_option().map(|contents| contents.items).unwrap_or_default();
        let count = items.len();
        rows.extend(items.into_iter().map(|item| item.uri.unwrap_or_default()));
        if count == 0 || rows.len() >= length {
            break;
        }
    }
    Ok((revision.unwrap_or_default(), rows))
}

async fn post<M: protobuf::MessageFull>(session: &Session, path: &str, message: &M) -> Result<Vec<u8>, librespot_core::Error> {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/x-protobuf"));
    let bytes = session.spclient().request_with_protobuf(&Method::POST, path, Some(headers), message).await?;
    Ok(bytes.to_vec())
}

/// The revision after a write: the last of resulting_revisions, otherwise the response's revision.
fn resulting_revision(bytes: &[u8]) -> Result<Vec<u8>, Failure> {
    let content = SelectedListContent::parse_from_bytes(bytes).map_err(|error| Failure::Retry(error.into()))?;
    Ok(content.resulting_revisions.last().cloned().or(content.revision).unwrap_or_default())
}

/// A refusal after which the list should be re-read and retried: the rows have diverged from the base.
fn conflict(error: &librespot_core::Error) -> bool {
    matches!(error.kind, ErrorKind::InvalidArgument | ErrorKind::FailedPrecondition | ErrorKind::Aborted | ErrorKind::Unknown)
}

fn status_of(kind: ErrorKind) -> Option<u16> {
    match kind {
        ErrorKind::InvalidArgument => Some(400),
        ErrorKind::Unauthenticated => Some(401),
        ErrorKind::PermissionDenied => Some(403),
        ErrorKind::NotFound => Some(404),
        ErrorKind::FailedPrecondition | ErrorKind::Aborted => Some(409),
        _ => None,
    }
}

/// A write error: a service refusal (4xx) is final — the Web API would answer the same; a lost response
/// of a non-retryable write too (it may have landed), everything else — let the Web API try.
fn written(error: librespot_core::Error, repeatable: bool) -> Failure {
    match status_of(error.kind) {
        Some(403) => Failure::Definitive(ApiError::Status {
            status: 403,
            message: "Spotify doesn't allow changing this playlist".into(),
        }),
        Some(status) => Failure::Definitive(ApiError::Status { status, message: String::new() }),
        None if !repeatable && matches!(error.kind, ErrorKind::Unavailable | ErrorKind::DeadlineExceeded) => {
            Failure::Definitive(ApiError::Network(error.to_string()))
        }
        None => error.into(),
    }
}

/// A Web API/session snapshot_id → a revision (base64 with or without padding).
pub fn revision_of(snapshot: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD_NO_PAD.decode(snapshot.trim_end_matches('=')).ok().filter(|bytes| !bytes.is_empty())
}

// --- operations ---------------------------------------------------------------------------------

fn item(uri: &str) -> Item {
    let mut item = Item::new();
    item.uri = Some(uri.to_owned());
    item
}

fn op(kind: op::Kind) -> Op {
    let mut op = Op::new();
    op.kind = Some(EnumOrUnknown::new(kind));
    op
}

fn list_changes(base: Vec<u8>, ops: Vec<Op>) -> ListChanges {
    let mut source = SourceInfo::new();
    source.client = Some(EnumOrUnknown::new(source_info::Client::CLIENT));
    let mut info = ChangeInfo::new();
    info.source = MessageField::some(source);
    let mut delta = Delta::new();
    delta.ops = ops;
    delta.info = MessageField::some(info);
    let mut changes = ListChanges::new();
    changes.base_revision = Some(base).filter(|base| !base.is_empty());
    changes.deltas = vec![delta];
    changes.want_resulting_revisions = Some(true);
    changes
}

/// Edit operations over the current rows `rows` (needed only for Remove).
fn ops_for(change: &Change, rows: &[String]) -> Vec<Op> {
    match change {
        Change::Add { uris, at } => add_ops(uris, *at),
        Change::Remove { uris } => removal_ops(rows, uris),
        Change::Move { from, insert_before, .. } => {
            if from == insert_before || from + 1 == *insert_before {
                return Vec::new();
            }
            let mut mov = Mov::new();
            mov.from_index = Some(*from as i32);
            mov.length = Some(1);
            mov.to_index = Some(*insert_before as i32);
            let mut op = op(op::Kind::MOV);
            op.mov = MessageField::some(mov);
            vec![op]
        }
        Change::Details { name, description, collaborative } => {
            if name.is_none() && description.is_none() && collaborative.is_none() {
                return Vec::new();
            }
            vec![attributes_op(name.as_deref(), description.as_deref(), *collaborative, None)]
        }
        Change::Picture(picture) => vec![attributes_op(None, None, None, Some(picture.clone()))],
    }
}

/// ADD in chunks: at the end or consecutively from position `at`.
fn add_ops(uris: &[String], at: Option<u32>) -> Vec<Op> {
    uris.chunks(ADD_CHUNK)
        .enumerate()
        .map(|(index, chunk)| {
            let mut add = Add::new();
            add.items = chunk.iter().map(|uri| item(uri)).collect();
            match at {
                Some(at) => add.from_index = Some((at as usize + index * ADD_CHUNK) as i32),
                None => add.add_last = Some(true),
            }
            let mut op = op(op::Kind::ADD);
            op.add = MessageField::some(add);
            op
        })
        .collect()
}

/// REM of all occurrences of `uris`: consecutive rows — in a single operation, from the end of the list, so that the indices
/// of later operations do not shift. The service checks the row URIs against the positions.
fn removal_ops(rows: &[String], uris: &[String]) -> Vec<Op> {
    let wanted: HashSet<&str> = uris.iter().map(String::as_str).collect();
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if !wanted.contains(row.as_str()) {
            continue;
        }
        match runs.last_mut() {
            Some((start, length)) if *start + *length == index => *length += 1,
            _ => runs.push((index, 1)),
        }
    }
    runs.into_iter()
        .rev()
        .map(|(start, length)| {
            let mut rem = Rem::new();
            rem.from_index = Some(start as i32);
            rem.length = Some(length as i32);
            rem.items = rows[start..start + length].iter().map(|uri| item(uri)).collect();
            let mut op = op(op::Kind::REM);
            op.rem = MessageField::some(rem);
            op
        })
        .collect()
}

/// UPDATE_LIST_ATTRIBUTES; an empty description — remove it (no_value).
fn attributes_op(name: Option<&str>, description: Option<&str>, collaborative: Option<bool>, picture: Option<Vec<u8>>) -> Op {
    let mut values = ListAttributes::new();
    let mut state = ListAttributesPartialState::new();
    values.name = name.map(str::to_owned);
    match description {
        Some("") => state.no_value.push(EnumOrUnknown::new(ListAttributeKind::LIST_DESCRIPTION)),
        Some(text) => values.description = Some(text.to_owned()),
        None => {}
    }
    values.collaborative = collaborative;
    values.picture = picture;
    state.values = MessageField::some(values);
    let mut update = UpdateListAttributes::new();
    update.new_attributes = MessageField::some(state);
    let mut op = op(op::Kind::UPDATE_LIST_ATTRIBUTES);
    op.update_list_attributes = MessageField::some(update);
    op
}

fn rootlist_add_op(uri: &str, public: Option<bool>) -> Op {
    let mut row = item(uri);
    if let Some(public) = public {
        let mut attributes = ItemAttributes::new();
        attributes.public = Some(public);
        row.attributes = MessageField::some(attributes);
    }
    let mut add = Add::new();
    add.items = vec![row];
    add.add_first = Some(true);
    let mut op = op(op::Kind::ADD);
    op.add = MessageField::some(add);
    op
}

fn rootlist_remove_op(uri: &str) -> Op {
    let mut rem = Rem::new();
    rem.items = vec![item(uri)];
    rem.items_as_key = Some(true);
    let mut op = op(op::Kind::REM);
    op.rem = MessageField::some(rem);
    op
}

fn public_op(index: usize, public: bool) -> Op {
    let mut values = ItemAttributes::new();
    values.public = Some(public);
    let mut state = ItemAttributesPartialState::new();
    state.values = MessageField::some(values);
    let mut update = UpdateItemAttributes::new();
    update.index = Some(index as i32);
    update.new_attributes = MessageField::some(state);
    let mut op = op(op::Kind::UPDATE_ITEM_ATTRIBUTES);
    op.update_item_attributes = MessageField::some(update);
    op
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uris(list: &[&str]) -> Vec<String> {
        list.iter().map(|uri| (*uri).to_owned()).collect()
    }

    fn rem_of(op: &Op) -> (i32, i32, Vec<String>) {
        assert_eq!(op.kind.unwrap().enum_value(), Ok(op::Kind::REM));
        let rem = op.rem.as_ref().unwrap();
        (rem.from_index(), rem.length(), rem.items.iter().map(|item| item.uri().to_owned()).collect())
    }

    #[test]
    fn every_copy_is_removed_from_the_end_in_runs() {
        let rows = uris(&["a", "b", "b", "c", "a", "d"]);
        let ops = removal_ops(&rows, &uris(&["a", "b"]));
        let runs: Vec<_> = ops.iter().map(rem_of).collect();
        assert_eq!(runs, vec![(4, 1, uris(&["a"])), (0, 3, uris(&["a", "b", "b"]))]);
        assert!(removal_ops(&rows, &uris(&["z"])).is_empty());
    }

    #[test]
    fn adds_go_last_or_from_a_position_in_chunks() {
        let many: Vec<String> = (0..ADD_CHUNK + 3).map(|n| format!("spotify:track:{n}")).collect();
        let ops = add_ops(&many, Some(7));
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].add.from_index(), 7);
        assert_eq!(ops[1].add.from_index(), (7 + ADD_CHUNK) as i32);
        assert_eq!(ops[1].add.items.len(), 3);
        let last = add_ops(&many[..1], None);
        assert!(last[0].add.add_last());
        assert_eq!(last[0].add.from_index, None);
    }

    #[test]
    fn a_move_is_written_as_insert_before_and_no_op_moves_are_skipped() {
        let ops = ops_for(&Change::Move { from: 0, insert_before: 2, snapshot: None }, &[]);
        let mov = ops[0].mov.as_ref().unwrap();
        assert_eq!((mov.from_index(), mov.length(), mov.to_index()), (0, 1, 2));
        assert!(ops_for(&Change::Move { from: 3, insert_before: 4, snapshot: None }, &[]).is_empty());
        assert!(ops_for(&Change::Move { from: 3, insert_before: 3, snapshot: None }, &[]).is_empty());
    }

    #[test]
    fn details_set_what_was_given_and_clear_an_empty_description() {
        let change = Change::Details { name: Some("New".into()), description: Some(String::new()), collaborative: Some(true) };
        let ops = ops_for(&change, &[]);
        let state = ops[0].update_list_attributes.new_attributes.as_ref().unwrap();
        assert_eq!(state.values.name(), "New");
        assert_eq!(state.values.description, None);
        assert_eq!(state.values.collaborative, Some(true));
        assert_eq!(state.no_value, vec![EnumOrUnknown::new(ListAttributeKind::LIST_DESCRIPTION)]);
        // the proto2 required fields are filled in — the message serializes
        assert!(list_changes(vec![1], ops).write_to_bytes().is_ok());
        let none = Change::Details { name: None, description: None, collaborative: None };
        assert!(ops_for(&none, &[]).is_empty());
    }

    #[test]
    fn rootlist_ops_carry_visibility_and_remove_by_key() {
        let add = rootlist_add_op("spotify:playlist:x", Some(false));
        assert!(add.add.add_first());
        assert_eq!(add.add.items[0].attributes.public, Some(false));
        let rem = rootlist_remove_op("spotify:playlist:x");
        assert!(rem.rem.items_as_key());
        assert_eq!(rem.rem.from_index, None);
        let public = public_op(3, true);
        let update = public.update_item_attributes.as_ref().unwrap();
        assert_eq!(update.index(), 3);
        // with item the service answers 400
        assert!(update.item.is_none());
        assert_eq!(update.new_attributes.values.public, Some(true));
    }

    #[test]
    fn a_snapshot_reads_back_as_its_revision() {
        let revision = vec![0, 0, 0, 7, 81, 7, 16, 94, 62, 49, 188, 112, 63, 244, 60, 107, 101, 51, 198, 229, 236, 21, 38, 139];
        let snapshot = session_reads::snapshot(&revision);
        assert_eq!(revision_of(&snapshot), Some(revision.clone()));
        assert_eq!(revision_of(&format!("{snapshot}==")), Some(revision));
        assert_eq!(revision_of(""), None);
        assert_eq!(revision_of("not base64"), None);
    }

    #[test]
    fn the_changes_name_their_base_and_ask_for_the_result() {
        let changes = list_changes(vec![1, 2], vec![rootlist_remove_op("spotify:playlist:x")]);
        assert_eq!(changes.base_revision(), [1, 2]);
        assert!(changes.want_resulting_revisions());
        assert_eq!(changes.deltas.len(), 1);
        assert_eq!(list_changes(Vec::new(), Vec::new()).base_revision, None);
    }

    #[test]
    fn a_mosaic_is_made_of_the_first_distinct_covers() {
        let url = |hex: &str| format!("{IMAGE_HOST}{hex}");
        let urls = [url("aa"), "https://elsewhere/x".to_owned(), url("aa"), url("bb"), String::new(), url("cc"), url("dd"), url("ee")];
        let images = mosaic_from(urls.iter().map(String::as_str));
        assert_eq!(images[0].url, "https://mosaic.scdn.co/640/aabbccdd");
        let few = mosaic_from([url("aa").as_str(), url("aa").as_str()]);
        assert_eq!(few.len(), 1);
        assert_eq!(few[0].url, url("aa"));
        assert!(mosaic_from([]).is_empty());
    }

    #[test]
    fn a_mosaic_needs_four_covers() {
        assert!(mosaic_images(&[]).is_empty());
        let one = mosaic_images(&uris(&["aa", "bb"]));
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].url, format!("{IMAGE_HOST}aa"));
        let four = mosaic_images(&uris(&["aa", "bb", "cc", "dd", "ee"]));
        assert_eq!(four.iter().map(|image| image.width).collect::<Vec<_>>(), vec![Some(640), Some(300), Some(60)]);
        assert_eq!(four[0].url, "https://mosaic.scdn.co/640/aabbccdd");
    }

    fn meta(name: &str, owner: &str, length: i32, collaborative: bool) -> MetaItem {
        let mut attributes = ListAttributes::new();
        attributes.name = Some(name.to_owned());
        attributes.description = Some("description".to_owned());
        attributes.collaborative = Some(collaborative);
        attributes.picture = Some(vec![0xab; 20]);
        let mut meta = MetaItem::new();
        meta.attributes = MessageField::some(attributes);
        meta.owner_username = Some(owner.to_owned());
        meta.length = Some(length);
        meta.revision = Some(vec![0, 0, 0, 2, 9]);
        meta
    }

    #[test]
    fn the_rootlist_reads_as_the_library_list_with_folders() {
        let mut page = ListItems::new();
        let mut row = |uri: &str, meta: MetaItem, public: Option<bool>| {
            let mut item = item(uri);
            if public.is_some() {
                let mut attributes = ItemAttributes::new();
                attributes.public = public;
                item.attributes = MessageField::some(attributes);
            }
            page.items.push(item);
            page.meta_items.push(meta);
        };
        row("spotify:playlist:aaa", meta("Mine", "me", 12, false), Some(true));
        row("spotify:start-group:f1:%D0%9F%D0%B0%D0%BF%D0%BA%D0%B0", MetaItem::new(), None);
        row("spotify:playlist:bbb", meta("Someone else's", "spotify", 50, true), None);
        let mut gone = meta("Deleted", "x", 1, false);
        gone.status_code = Some(404);
        row("spotify:playlist:ccc", gone, None);
        row("spotify:end-group:f1", MetaItem::new(), None);
        let library = library_of(&[page]);
        assert_eq!(library.rootlist.entries.len(), 5);
        let ids: Vec<&str> = library.playlists.iter().map(|playlist| playlist.id.as_str()).collect();
        assert_eq!(ids, ["aaa", "bbb"]);
        let mine = &library.playlists[0];
        assert_eq!(mine.name, "Mine");
        assert_eq!(mine.uri, "spotify:playlist:aaa");
        assert_eq!(mine.description.as_deref(), Some("description"));
        assert_eq!(mine.owner.id.as_deref(), Some("me"));
        assert_eq!(mine.owner.uri.as_deref(), Some("spotify:user:me"));
        assert_eq!(mine.public, Some(true));
        assert_eq!(mine.track_total(), 12);
        assert_eq!(mine.snapshot_id.as_deref(), Some(session_reads::snapshot(&[0, 0, 0, 2, 9]).as_str()));
        assert_eq!(mine.images[0].url, format!("{IMAGE_HOST}{}", "ab".repeat(20)));
        assert!(library.playlists[1].collaborative);
        assert_eq!(library.playlists[1].public, None);
    }
}

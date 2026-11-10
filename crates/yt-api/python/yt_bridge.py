#!/usr/bin/env python3
"""ytmusicapi bridge for Materialfy-YT (fork of fatyzzz/materialfy).

One-shot CLI: python3 yt_bridge.py --headers <headers.json> --method <m> --args '<json>'
Prints a single JSON object: {"ok": true, "data": ...} or {"ok": false, "error": "..."}.

Self-contained: ytmusicapi is vendored next to this file in ./vendor
(https://github.com/sigma67/ytmusicapi, MIT) — no `pip install` needed.
Python >= 3.10.
"""
import argparse
import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "vendor"))

try:
    from ytmusicapi import YTMusic
    from ytmusicapi.auth.types import AuthType
except ImportError:
    YTMusic = None
    AuthType = None


def account_info(yt) -> dict:
    """Signed-in channel profile: name, avatar, channel id.

    The liked-songs playlist ("LM") belongs to the signed-in channel, so its
    author is the account. Avatar comes from the channel's artist page.
    """
    pl = yt.get_playlist("LM", limit=1)
    author = pl.get("author") or {}
    name = author.get("name") or ""
    chid = author.get("id") or ""
    avatar = ""
    if chid:
        try:
            info = yt.get_artist(chid)
            thumbs = info.get("thumbnails") or []
            if thumbs:
                avatar = thumbs[-1].get("url", "") if isinstance(thumbs[-1], dict) else ""
        except Exception:
            avatar = ""
    return {"name": name, "avatar": avatar, "channelId": chid}


def lyric_line(item):
    """Normalize a timestamped lyric (LyricLine dataclass or plain dict) to JSON-safe fields."""
    if isinstance(item, dict):
        return {"text": item.get("text", ""), "startTime": item.get("start_time"), "endTime": item.get("end_time")}
    return {"text": getattr(item, "text", ""), "startTime": getattr(item, "start_time", None), "endTime": getattr(item, "end_time", None)}


def grid_items(resp: dict) -> list:
    """All grid items from a singleColumnBrowseResultsRenderer response."""
    try:
        tabs = resp["contents"]["singleColumnBrowseResultsRenderer"]["tabs"]
        items = []
        for tab in tabs:
            contents = tab["tabRenderer"]["content"]["sectionListRenderer"]["contents"]
            for section in contents:
                for item in section.get("gridRenderer", {}).get("items", []):
                    items.append(item)
        return items
    except (KeyError, TypeError):
        return []


def json_safe(value):
    """Convert ytmusicapi dataclasses (Description, etc.) to JSON-safe values."""
    if isinstance(value, dict):
        return {k: json_safe(v) for k, v in value.items()}
    if isinstance(value, list):
        return [json_safe(v) for v in value]
    if hasattr(value, "text"):
        return value.text
    return value


def two_row_item(item: dict) -> dict:
    """Normalize a musicTwoRowItemRenderer item into a plain dict."""
    row = item.get("musicTwoRowItemRenderer", {}) if isinstance(item, dict) else {}
    if not row:
        return {}
    def run_texts(node):
        return [r.get("text", "") for r in node.get("runs", []) if isinstance(r, dict)]
    title = "".join(run_texts(row.get("title", {}))).strip()
    subtitle = "".join(run_texts(row.get("subtitle", {}))).strip()
    nav = row.get("navigationEndpoint", {})
    browse_id = (nav.get("browseEndpoint") or {}).get("browseId", "")
    playlist_id = (nav.get("watchPlaylistEndpoint") or {}).get("playlistId", "")
    thumbs = row.get("thumbnailRenderer", {}).get("musicThumbnailRenderer", {}).get("thumbnail", {}).get("thumbnails", [])
    cover = ""
    if thumbs:
        biggest = max(thumbs, key=lambda t: t.get("width", 0) if isinstance(t, dict) else 0)
        cover = biggest.get("url", "") if isinstance(biggest, dict) else ""
    return {
        "title": title,
        "subtitle": subtitle,
        "browseId": browse_id,
        "playlistId": playlist_id,
        "cover": cover,
    }


def session_is_signed_in(yt) -> bool:
    """True when the headers belong to a signed-in account.

    A guest cookie (or a header file whose session expired) is answered by
    YouTube with an empty library and an account menu without the account
    header. Only a real session renders the active account header there.
    """
    if yt.auth_type == AuthType.UNAUTHORIZED:
        return False
    try:
        response = yt._send_request("account/account_menu", {})
    except Exception:
        return False
    return _has_key(response, "activeAccountHeaderRenderer")


def _has_key(node, key: str) -> bool:
    """True when `key` appears anywhere in the nested response."""
    if isinstance(node, dict):
        if key in node:
            return True
        return any(_has_key(value, key) for value in node.values())
    if isinstance(node, list):
        return any(_has_key(item, key) for item in node)
    return False

def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--headers", default=None)
    ap.add_argument("--method", required=True)
    ap.add_argument("--args", default="{}")
    ns = ap.parse_args()

    try:
        from ytmusicapi import YTMusic
    except ImportError:
        print(json.dumps({
            "ok": False,
            "error": "ytmusicapi is not installed. Run: pip install ytmusicapi",
        }))
        return 0

    args = json.loads(ns.args or "{}")
    try:
        yt = YTMusic(ns.headers) if ns.headers else YTMusic()
    except Exception as e:  # noqa: BLE001 - surfaced as JSON
        print(json.dumps({"ok": False, "error": f"YTMusic init failed: {e}"}))
        return 0

    m = ns.method
    try:
        if m == "search":
            data = yt.search(args.get("query", ""), filter=args.get("filter"),
                             limit=args.get("limit", 20))
        elif m == "search_suggestions":
            data = yt.get_search_suggestions(args.get("query", ""))
        elif m == "get_home":
            data = yt.get_home(limit=args.get("limit", 3))
        elif m == "get_new_releases":
            # "New releases" browse: albums/singles (signed in) or release
            # playlists (guest). Normalize two-row items into plain dicts.
            resp = yt._send_request("browse", {"browseId": "FEmusic_new_releases"})
            data = {"items": [two_row_item(it) for it in grid_items(resp)]}
        elif m == "get_library_playlists":
            data = yt.get_library_playlists(limit=args.get("limit", 25))
        elif m == "get_account":
            data = account_info(yt)
        elif m == "check_session":
            # Is this cookie a signed-in session or just a guest cookie?
            # A guest cookie returns an empty library instead of an auth error,
            # so the library alone cannot answer this. The account menu does:
            # a signed-in session renders the active account header.
            data = {"signedIn": session_is_signed_in(yt)}
        elif m == "get_liked_songs":
            data = yt.get_liked_songs(limit=args.get("limit", 50))
        elif m == "get_library_songs":
            data = yt.get_library_songs(limit=args.get("limit", 50))
        elif m == "get_library_albums":
            data = yt.get_library_albums(limit=args.get("limit", 50))
        elif m == "get_library_artists":
            data = yt.get_library_artists(limit=args.get("limit", 50))
        elif m == "get_library_podcasts":
            data = yt.get_library_podcasts(limit=args.get("limit", 50))
        elif m == "subscribe_artist":
            data = yt.subscribe_artist(args["channelId"])
        elif m == "unsubscribe_artist":
            data = yt.unsubscribe_artist(args["channelId"])
        elif m == "get_library_subscriptions":
            data = yt.get_library_subscriptions(limit=args.get("limit", 25))
        elif m == "get_playlist":
            data = yt.get_playlist(args["playlistId"], limit=args.get("limit", 100))
        elif m == "create_playlist":
            # returns the new playlistId (or a dict with it)
            data = yt.create_playlist(
                title=args.get("title", ""),
                description=args.get("description", ""),
                privacy_status=args.get("privacyStatus", "PRIVATE"),
                video_ids=args.get("videoIds") or None,
                source_playlist=args.get("sourcePlaylist") or None,
            )
        elif m == "edit_playlist":
            data = yt.edit_playlist(
                args["playlistId"],
                title=args.get("title") or None,
                description=args.get("description") or None,
                privacyStatus=args.get("privacyStatus") or None,
                collaboration=args.get("collaborative"),
                moveItem=tuple(args["moveItem"]) if args.get("moveItem") else None,
                addToTop=bool(args.get("addToTop", False)),
            )
        elif m == "delete_playlist":
            data = yt.delete_playlist(args["playlistId"])
        elif m == "add_playlist_items":
            data = yt.add_playlist_items(
                args["playlistId"],
                videoIds=args.get("videoIds") or None,
                source_playlist=args.get("sourcePlaylist") or None,
                duplicates=bool(args.get("duplicates", False)),
            )
        elif m == "remove_playlist_items":
            # every entry needs videoId + setVideoId (present for owned playlists)
            data = yt.remove_playlist_items(args["playlistId"], args.get("videos") or [])
        elif m == "rate_playlist":
            data = yt.rate_playlist(args["playlistId"], args.get("rating", "INDIFFERENT"))
        elif m == "get_album":
            data = yt.get_album(args["browseId"])
        elif m == "get_artist":
            data = yt.get_artist(args["channelId"])
        elif m == "get_song":
            data = yt.get_song(args["videoId"])
        elif m == "download_audio":
            # Google Video caps plain `url` downloads at 1 MiB per token (verified
            # against the CDN), so hand the byte fetch to yt-dlp (vendored) which
            # does the JS `n` deciphering that lifts the cap. Progress goes to
            # stderr via the redirect, so the stdout line stays a clean JSON object.
            import contextlib
            import io

            import yt_dlp
            vid = args["videoId"]
            out = args.get("output") or f"/tmp/yt_{vid}.m4a"
            os.makedirs(os.path.dirname(out) or ".", exist_ok=True)
            opts = {
                "outtmpl": out,
                "format": "bestaudio[ext=m4a]/bestaudio[ext=webm]/bestaudio",
                "quiet": True,
                "no_warnings": True,
                "noplaylist": True,
                "noprogress": True,
                "progress_hooks": [],
            }
            sink = io.StringIO()
            with contextlib.redirect_stdout(sink), contextlib.redirect_stderr(sink):
                with yt_dlp.YoutubeDL(opts) as ydl:
                    ydl.download([f"https://www.youtube.com/watch?v={vid}"])
            data = {"file": out}
        elif m == "get_watch_playlist":
            data = yt.get_watch_playlist(
                videoId=args.get("videoId"),
                playlistId=args.get("playlistId"),
                radio=bool(args.get("radio", False)),
                shuffle=bool(args.get("shuffle", False)),
                limit=args.get("limit", 25),
            )
        elif m == "get_lyrics":
            # get_lyrics takes a lyrics browseId (MPLYt...), not a videoId.
            # The browseId for a video comes from its watch playlist.
            vid = args["videoId"]
            wl = yt.get_watch_playlist(videoId=vid, limit=1)
            browse_id = wl.get("lyrics") if isinstance(wl, dict) else None
            data = None
            if browse_id and str(browse_id).startswith("MPLY"):
                result = yt.get_lyrics(browse_id, timestamps=True)
                if isinstance(result, dict) and result.get("hasTimestamps") and isinstance(result.get("lyrics"), list):
                    data = {"lines": [lyric_line(l) for l in result["lyrics"]],
                            "source": result.get("source", ""), "hasTimestamps": True}
                elif result:
                    data = result
            elif browse_id:
                data = yt.get_lyrics(browse_id)
        elif m == "get_channel":
            data = yt.get_channel(args["channelId"])
        elif m == "get_album_browse_id":
            data = yt.get_album_browse_id(args["audioPlaylistId"])
        elif m == "get_podcast":
            data = yt.get_podcast(args["playlistId"], limit=args.get("limit", 100))
        elif m == "get_episode":
            data = json_safe(yt.get_episode(args["videoId"]))
        elif m == "rate_song":
            yt.rate_song(args["videoId"], args.get("rating", "dislike"))
            data = {"ok": True}
        else:
            print(json.dumps({"ok": False, "error": f"unknown method {m}"}))
            return 0
        print(json.dumps({"ok": True, "data": data}))
    except Exception as e:  # noqa: BLE001
        print(json.dumps({"ok": False, "error": str(e)}))
    return 0


if __name__ == "__main__":
    sys.exit(main())

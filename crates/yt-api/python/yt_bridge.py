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
        elif m == "get_library_playlists":
            data = yt.get_library_playlists(limit=args.get("limit", 25))
        elif m == "get_account":
            # Profile from the liked-songs playlist (LM): its author is the
            # signed-in channel. Avatar comes from get_artist on that channel.
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
            data = {"name": name, "avatar": avatar, "channelId": chid}
        elif m == "get_liked_songs":
            data = yt.get_liked_songs(limit=args.get("limit", 50))
        elif m == "get_library_songs":
            data = yt.get_library_songs(limit=args.get("limit", 50))
        elif m == "get_library_albums":
            data = yt.get_library_albums(limit=args.get("limit", 50))
        elif m == "get_library_artists":
            data = yt.get_library_artists(limit=args.get("limit", 50))
        elif m == "get_library_subscriptions":
            data = yt.get_library_subscriptions(limit=args.get("limit", 25))
        elif m == "get_playlist":
            data = yt.get_playlist(args["playlistId"], limit=args.get("limit", 100))
        elif m == "get_album":
            data = yt.get_album(args["browseId"])
        elif m == "get_artist":
            data = yt.get_artist(args["channelId"])
        elif m == "get_song":
            data = yt.get_song(args["videoId"])
        elif m == "get_stream_url":
            # The signed-in WEB_REMIX request returns only signatureCipher formats,
            # which need JS signature deciphering before Google will serve them
            # (HTTP 403). The anonymous ANDROID player returns plain `url` formats
            # directly, so resolve the playback URL that way.
            import requests
            vid = args["videoId"]
            payload = {
                "context": {
                    "client": {
                        "clientName": "ANDROID",
                        "clientVersion": "20.10.38",
                        "hl": "en",
                        "gl": "US",
                        "androidSdkVersion": 34,
                        "osVersion": "14",
                        "deviceMake": "Google",
                        "deviceModel": "Pixel 9",
                    }
                },
                "videoId": vid,
                "playbackContext": {"contentPlaybackContext": {"signatureTimestamp": "20000"}},
                "contentCheckOk": True,
                "racyCheckOk": True,
            }
            r = requests.post(
                "https://www.youtube.com/youtubei/v1/player?key=AIzaSyAO_FJ2SlqU8Q4STEHLGCilw_Y9_11qcW8",
                json=payload,
                timeout=25,
            )
            if r.status_code != 200:
                raise RuntimeError(f"player request failed: HTTP {r.status_code}")
            sd = (r.json().get("streamingData") or {})
            best = None
            for f in sd.get("adaptiveFormats") or []:
                mt = f.get("mimeType") or ""
                if not mt.startswith("audio/"):
                    continue
                url = f.get("url") or ""
                if not url:
                    continue
                usable = mt.startswith("audio/mp4")
                bitrate = f.get("bitrate") or 0
                if best is None or (usable, bitrate) > (best[0], best[1]):
                    best = (usable, bitrate, url)
            if best is None:
                raise RuntimeError("No playable audio stream (sign-in or age restriction?)")
            data = {"url": best[2], "bitrate": best[1]}
        elif m == "download_audio":
            # Google Video caps plain `url` downloads at ~1 MiB per token for
            # this egress, so hand the byte fetch to yt-dlp (vendored, latest)
            # which handles manifest/fragment/ABR download properly.
            import io
            import contextlib
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
            data = yt.get_lyrics(args["videoId"])
        elif m == "get_channel":
            data = yt.get_channel(args["channelId"])
        elif m == "get_album_browse_id":
            data = yt.get_album_browse_id(args["audioPlaylistId"])
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

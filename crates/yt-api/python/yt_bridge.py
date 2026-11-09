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

"""Tests for the bridge's session check.

A guest cookie answers YouTube's library calls with an empty list instead of
an auth error, so the library cannot tell a guest from a signed-in account.
The account menu can: only a real session renders the active account header.

Run: python3 crates/yt-api/python/tests/test_bridge.py
"""
import os
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, ".."))
sys.path.insert(0, os.path.join(HERE, "..", "vendor"))

import yt_bridge


class _AuthType:
    UNAUTHORIZED = "UNAUTHORIZED"
    BROWSER = "BROWSER"


class _FakeYT:
    """Stands in for YTMusic: answers the account menu with a canned response."""

    def __init__(self, response, auth_type=_AuthType.BROWSER, raises=False):
        self._response = response
        self._raises = raises
        self.auth_type = auth_type

    def _send_request(self, endpoint, body):
        if self._raises:
            raise RuntimeError("boom")
        return self._response


SIGNED_IN = {
    "actions": [
        {
            "openPopupAction": {
                "popup": {
                    "multiPageMenuRenderer": {
                        "header": {
                            "activeAccountHeaderRenderer": {
                                "accountName": {"runs": [{"text": "Real Person"}]},
                                "channelHandle": {"runs": [{"text": "@realperson"}]},
                            }
                        }
                    }
                }
            }
        }
    ]
}

GUEST = {
    "actions": [
        {
            "openPopupAction": {
                "popup": {
                    "multiPageMenuRenderer": {
                        "sections": [
                            {
                                "multiPageMenuSectionRenderer": {
                                    "items": [
                                        {
                                            "compactLinkRenderer": {
                                                "title": {"runs": [{"text": "Get Music Premium"}]}
                                            }
                                        }
                                    ]
                                }
                            }
                        ]
                    }
                }
            }
        }
    ]
}


class SessionCheckTest(unittest.TestCase):
    def setUp(self):
        # The bridge reads AuthType from ytmusicapi; the fake uses its own.
        yt_bridge.AuthType = _AuthType

    def test_signed_in_account_is_accepted(self):
        self.assertTrue(yt_bridge.session_is_signed_in(_FakeYT(SIGNED_IN)))

    def test_guest_account_is_rejected(self):
        self.assertFalse(yt_bridge.session_is_signed_in(_FakeYT(GUEST)))

    def test_unauthorized_client_is_rejected(self):
        yt = _FakeYT(SIGNED_IN, auth_type=_AuthType.UNAUTHORIZED)
        self.assertFalse(yt_bridge.session_is_signed_in(yt))

    def test_failed_request_is_rejected(self):
        yt = _FakeYT(SIGNED_IN, raises=True)
        self.assertFalse(yt_bridge.session_is_signed_in(yt))


if __name__ == "__main__":
    unittest.main()

"""Transport-only Ubuntu mirror adapter for frozen replay jobs.

The frozen acquirers continue to construct and validate snapshot URLs.  CI may
set ``BOOLE_UBUNTU_MIRROR_ARCH`` to fetch those exact, hash-pinned bytes from
Ubuntu's official architecture mirror when the historical snapshot service is
unavailable.  The wrapped response keeps the original snapshot URL as its
identity, so every existing size, digest, and no-redirect check still runs.
"""

from __future__ import annotations

import os
import pathlib
import sys
import urllib.error
import urllib.request
from collections.abc import Callable
from typing import Any


REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts.native_shadow_official_mirror_seed_v1 import mirror_url


class _SnapshotIdentityResponse:
    def __init__(self, response: Any, original_url: str) -> None:
        self._response = response
        self._original_url = original_url

    def geturl(self) -> str:
        return self._original_url

    def __enter__(self) -> "_SnapshotIdentityResponse":
        enter = getattr(self._response, "__enter__", None)
        if enter is not None:
            enter()
        return self

    def __exit__(self, exc_type: Any, exc: Any, traceback: Any) -> Any:
        exit_method = getattr(self._response, "__exit__", None)
        if exit_method is not None:
            return exit_method(exc_type, exc, traceback)
        self._response.close()
        return None

    def __getattr__(self, name: str) -> Any:
        return getattr(self._response, name)


def adapted_open(original_open: Callable[..., Any], architecture: str) -> Callable[..., Any]:
    """Return an opener that changes transport while preserving authority."""

    if architecture not in ("amd64", "arm64"):
        raise RuntimeError("official Ubuntu mirror architecture differs")

    def open_with_official_mirror(
        opener: Any,
        fullurl: Any,
        *args: Any,
        **kwargs: Any,
    ) -> Any:
        if not isinstance(fullurl, str):
            return original_open(opener, fullurl, *args, **kwargs)
        try:
            transport_url = mirror_url(fullurl, architecture)
        except ValueError:
            return original_open(opener, fullurl, *args, **kwargs)
        try:
            response = original_open(opener, transport_url, *args, **kwargs)
        except (urllib.error.URLError, TimeoutError, OSError):
            # The architecture mirror is transport only.  If it no longer
            # publishes a frozen object (or is temporarily unavailable), use
            # the original timestamped snapshot endpoint.  The unchanged
            # acquirer still enforces the sealed size, digest, redirect and
            # response-identity checks after either transport succeeds.
            return original_open(opener, fullurl, *args, **kwargs)
        return _SnapshotIdentityResponse(response, fullurl)

    return open_with_official_mirror


def install_from_environment() -> None:
    architecture = os.environ.get("BOOLE_UBUNTU_MIRROR_ARCH")
    if architecture is None:
        return
    if getattr(urllib.request.OpenerDirector.open, "_boole_official_mirror_v1", False):
        return
    installed = adapted_open(urllib.request.OpenerDirector.open, architecture)
    setattr(installed, "_boole_official_mirror_v1", True)
    urllib.request.OpenerDirector.open = installed


install_from_environment()

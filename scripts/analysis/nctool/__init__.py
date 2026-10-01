"""nctool — the nc conversion-analysis toolkit.

The command groups are `manifest` (asset inventory), `compare` (the build-version
harness), `roll` (manifest-driven calibration, conversion, and deterministic
analysis artifacts), `metrics` (pixel-derived measurement of one converted image,
whatever produced it), `review` (render a described matrix of conversions into
a review set for `tools/review-app`, measuring each rendition as it goes), and
`acceptance` (decode every output encoding without nc and check it against the
buffers nc's encoders received).

All but `metrics` and `acceptance` are stdlib-only, because they read derived JSON
and stream checksums rather than loading pixels into Python. Those two need `numpy`,
`tifffile` and `Pillow` (`scripts/analysis/requirements.txt`), imported inside the
functions that touch pixels so that importing this package stays free.
"""

__all__ = ["acceptance", "compare", "manifest", "metrics", "review", "roll"]

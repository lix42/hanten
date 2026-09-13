"""nctool — the nc conversion-analysis toolkit.

The command groups are `manifest` (asset inventory), `compare` (the build-version
harness), `roll` (manifest-driven calibration, conversion, and deterministic
analysis artifacts), `metrics` (pixel-derived measurement of one converted image,
whatever produced it), and `review` (render a described matrix of conversions into
a review set for `tools/review-app`, measuring each rendition as it goes).

All but `metrics` are stdlib-only, because they read derived JSON and stream
checksums rather than loading pixels into Python. `metrics` needs `numpy`,
`tifffile` and `Pillow` (`scripts/analysis/requirements.txt`), imported inside the
functions that touch pixels so that importing this package stays free.
"""

__all__ = ["compare", "manifest", "metrics", "review", "roll"]

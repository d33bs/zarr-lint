# `zarr-lint fmt`

`zarr-lint fmt` formats local Zarr metadata files.

It changes JSON representation only. It does not rewrite chunk data, change
codecs, migrate Zarr versions, repair invalid metadata, or change the meaning of
the store.

## Commands

Preview changes without writing files:

```bash
zarr-lint fmt path/to/store.zarr
```

Fail if formatting is needed:

```bash
zarr-lint fmt path/to/store.zarr --check
```

Apply formatting changes:

```bash
zarr-lint fmt path/to/store.zarr --write
```

Emit a machine-readable report:

```bash
zarr-lint fmt path/to/store.zarr --format json
```

## Safety Rules

- Dry-run is the default.
- `--write` is required to modify files.
- Only recognized Zarr metadata files are formatted.
- Unrelated JSON files are ignored.
- Parsed JSON must be equal before and after formatting.
- Running `fmt` twice must produce no second change.

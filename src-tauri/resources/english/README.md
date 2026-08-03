# English Dictionary Backup

This directory stores an offline backup of the dictionary payload from:

- <https://isdc.pages.dev/>

The backup contains only the contents of the site's `asp-data` JSON script
element. It does not contain the site's interface, JavaScript bundles, or
other page resources.

## Files

- `isdc-asp-data.txt`: the 16-line Base85/Brotli dictionary payload.
- `SOURCE.json`: source URL, download time, size, and SHA-256 checksums.

The payload is kept as a backup copy only. The current English feature still
uses user-triggered downloads and local indexing, and the payload is not
automatically bundled into the Tauri installer. A later offline-first change
can wrap this payload in the expected `asp-data` marker and pass it to the
existing Rust decoder.

The source and permission information are recorded in
`src/features/english/SOURCE.md`. If the upstream author changes the license
or asks for removal, remove this backup and its integration promptly.

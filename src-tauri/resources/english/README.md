# English Dictionary Backup

This directory stores an offline backup of the dictionary payload from:

- <https://isdc.pages.dev/>

The backup contains only the contents of the site's `asp-data` JSON script
element. It does not contain the site's interface, JavaScript bundles, or
other page resources.

## Files

- `isdc-asp-data.txt`: the 16-line Base85/Brotli dictionary payload.
- `SOURCE.json`: source URL, download time, size, and SHA-256 checksums.

The payload is kept as a user-triggered backup copy. The English feature first
tries the source site, then GitHub Raw, and finally reads this bundled copy when
both network sources are unavailable. It is indexed into the user's app-data
directory only after the user clicks the install button.

The source and permission information are recorded in
`src/features/english/SOURCE.md`. If the upstream author changes the license
or asks for removal, remove this backup and its integration promptly.

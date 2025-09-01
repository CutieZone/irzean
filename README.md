# irzean

A handcrafted, zero-JS, Rust-powered site generator that renders on-demand like a dynamic site, but performs like it was pre-rendered.

Or... "Renders like a server, loads like a static site"

## envvars

- `IRZEAN_PORT` (default `1337`): the port to run on
- `IRZEAN_UPDATE_INTERVAL` (default `60`): how many seconds to wait between each update (whole number)
- `IRZEAN_PARENTAL_MODE` (default unset): when set, filters out NSFW content
- `IRZEAN_ACCESS_TOKEN` (required): access token to access the repository
- `IRZEAN_REPO_URL` (required): where Irzean gets its content from
- `IRZEAN_CLONE_PATH` (required): where Irzean will store its data
- `IRZEAN_ROOT_URL` (default `http://0.0.0.0:${IRZEAN_PORT:1337}`): the root url where Irzean is ran

Technically, `IRZEAN_CLONE_PATH` isn't required, and the container can be ran completely ephemerally...

**BUT** there's some flaws with that. Mostly caching.

## actual Features

- serverside rendered w/ `axum` + `minijinja`
- stupid fast loads (<30ms request processing time, <200ms network time, <0.5s FCP (measured from Germany to Eastern Europe))
- no js (other than umami) yet we have full text search
- tag index, listing, sitemap generator
- exists almost entirely in memory at all times
- `rust-embed` for static non-code
- ~14MiB memory usage with my writings directory and setup

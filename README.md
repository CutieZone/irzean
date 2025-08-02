# irzean

A shitty microblogging piece of trash.

## envvars

- `IRZEAN_PORT` (default `1337`): the port to run on
- `IRZEAN_UPDATE_INTERVAL` (default `60`): how many seconds to wait between each update (whole number)
- `IRZEAN_PARENTAL_MODE` (default unset): when set, filters out NSFW content
- `IRZEAN_ACCESS_TOKEN` (required): access token to access the repository
- `IRZEAN_REPO_URL` (required): where Irzean gets its content from
- `IRZEAN_CLONE_PATH` (required): where Irzean will store its data
- `IRZEAN_ROOT_URL` (default `http://0.0.0.0:${IRZEAN_PORT:1337}`): the root url where Irzean is ran

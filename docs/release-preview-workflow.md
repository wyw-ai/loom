# Versioned release preview workflow

Loom uses versioned release-preview branches. The unversioned `preview` branch
is retired.

## Normal flow

1. Feature and fix pull requests target the current `preview-X.Y.Z` candidate.
2. The candidate has a pull request into `main`; a human reviews the checks and
   decides when to merge it.
3. A merge into `main` triggers `Release preview rollover`.
4. The workflow inspects every valid `preview-X.Y.Z` branch on the current
   major release line (`preview-0.x.*` while Loom is pre-1.0). If a newer
   candidate already exists, it reuses that branch and only ensures its pull
   request exists.
5. If no newer candidate exists, it creates the next minor candidate from the
   new `main`, resets patch to zero, synchronizes every release version, pushes
   the branch, and opens its pull request. For example, merging
   `preview-0.1.8` creates `preview-0.2.0`.

The workflow ignores pushes to `main` that did not come from a merged
`preview-X.Y.Z` pull request. It can be run manually from Actions to recover an
interrupted rollover. A concurrency group prevents two merges or recovery runs
from creating the same candidate.

## Documentation and CHANGELOG policy for version improvements

Founder ruling (2026-08-15): `docs/context-layer/` and `CHANGELOG.md` stay
tracked in git, and every future major version improvement (for example,
merging a `preview-X.Y.Z` candidate into `main`) follows the rules below.

- Behavioral or on-disk format changes must be recorded under `[Unreleased]`
  in `CHANGELOG.md` (Keep a Changelog categories) in the same change set,
  before the improvement ships.
- Documentation under `docs/` that describes the changed behavior is updated
  in the same change set and stays git-tracked; new `docs/` subtrees require
  an explicit `.gitignore` whitelist entry so the docs and the code they
  describe cannot drift apart.
- When a `preview-X.Y.Z` candidate merges into `main`, the merge owner checks
  that accumulated `[Unreleased]` entries cover the candidate's behavioral
  changes before the rollover creates the next candidate.

## One-time bot setup

The `wyw-ai` organization does not allow the built-in `GITHUB_TOKEN` to create
pull requests. Install a repository-scoped GitHub App with these repository
permissions:

- Contents: read and write
- Pull requests: read and write

Then configure:

- Repository variable `LOOM_RELEASE_APP_ID`: the GitHub App ID
- Repository secret `LOOM_RELEASE_APP_PRIVATE_KEY`: the GitHub App private key

The workflow requests only those two write permissions from the installation
token. It never approves or merges its own pull request.

# Story 3.3: Cover Image Management

Status: done

## Story

As a librarian,
I want cover images to be downloaded, resized, and stored locally when metadata is fetched,
so that my catalog displays book and media covers reliably without depending on external URLs.

## Acceptance Criteria

### AC1: Cover Image Download (FR14)

- Given a metadata provider returns a `cover_url` in its response
- When the background metadata fetch task processes the result
- Then the system downloads the image from the URL using the shared `reqwest::Client`
- And validates the URL starts with `https://` (rewrite `http://` to `https://`)
- And validates the response is a valid image (Content-Type check or magic bytes)
- And if download fails (timeout, 404, invalid image): logs `tracing::warn!` and sets `cover_image_url = NULL` — no crash, no blocking

### AC2: Cover Image Resize (FR15)

- Given a cover image has been downloaded successfully
- When the image is processed
- Then it is resized to a maximum width of 400px while maintaining aspect ratio
- And encoded as JPEG with 80% quality
- And saved to `{covers_dir}/{title_id}.jpg`
- And the average file size is under 100KB
- And images already at or below 400px width are NOT upscaled but still re-encoded as JPEG 80% (format normalization)

### AC3: Local Cover Storage (AR11, NFR24)

- Given cover images need persistent storage
- When the application starts
- Then the covers directory path is read from `COVERS_DIR` environment variable (default: `./covers`)
- And the directory is created if it doesn't exist
- And cover images are served via tower-http `ServeDir` at the `/covers/` HTTP path
- And `cover_image_url` in the database is updated to `/covers/{title_id}.jpg` (local path, not remote URL)

### AC4: Cover Component Template (UX-DR10, FR88)

- Given a title may or may not have a cover image
- When the cover is rendered in any template
- Then the Cover component displays one of 3 states:
  - **Loading**: shimmer animation in a fixed 2:3 aspect ratio container (while metadata fetch is pending)
  - **Missing**: media-type SVG placeholder icon centered in muted background (when `cover_image_url` is NULL)
  - **Loaded**: `<img>` with `object-fit: cover`, `alt="Cover of {title}"`, rounded corners
- And the component supports 4 size variants:
  - Thumbnail: 40x60px (list rows, search results)
  - Card: 120x180px (browse list)
  - Detail: 200x300px (title detail page)
  - Grid: 150x225px (browse grid)
- And dark mode adds a light shadow around loaded images

### AC5: Lazy Loading

- Given cover images are rendered in templates
- When images are below the fold (browse grid, search results beyond initial viewport)
- Then the `<img>` tag includes `loading="lazy"` attribute
- And above-the-fold images (title detail page, first visible items) use `loading="eager"`

### AC6: Cover Component Integration

- Given the Cover component is implemented
- When it replaces existing cover display code
- Then it is used in: title detail page, home page search results, feedback entries (skeleton → resolved)
- And existing templates that render `cover_image_url` directly are updated to use the component
- And the component gracefully handles the transition from remote URL (pre-3-3 data) to local path

### AC7: Error Resilience

- Given cover download or processing may fail
- When any error occurs (network timeout, invalid image format, disk full, corrupt data)
- Then the error is logged with `tracing::warn!` and structured fields (title_id, cover_url, error)
- And `cover_image_url` is set to NULL in the database
- And the Cover component displays the Missing state (SVG placeholder)
- And the scan workflow is never blocked by cover failures

## Tasks / Subtasks

- [x] Task 1: Cover Download & Resize Service (AC: #1, #2, #7) — **CORE INFRASTRUCTURE**
  - [ ] Create `src/services/cover.rs` with `CoverService`
  - [ ] Add `pub mod cover;` to `src/services/mod.rs`
  - [ ] Method: `async fn download_and_resize(client: &reqwest::Client, cover_url: &str, title_id: u64, covers_dir: &Path) -> Result<String, CoverError>`
  - [ ] Download: use shared `reqwest::Client`, rewrite `http://` to `https://` on URL scheme
  - [ ] Image validation: use `image` crate decode — if `ImageReader::decode()` fails, the bytes are not a valid image
  - [ ] Resize: `ImageReader::new(Cursor::new(bytes)).with_guessed_format()?.decode()?`
  - [ ] If width > 400: `img.resize(400, u32::MAX, FilterType::Lanczos3)`; else keep original dimensions (no upscaling)
  - [ ] All images re-encoded as JPEG 80% regardless of input format (PNG→JPEG, JPEG→JPEG, etc.)
  - [ ] Encode: `image::codecs::jpeg::JpegEncoder::new_with_quality(writer, 80)`, then `resized.write_with_encoder(encoder)?`
  - [ ] Save to: `{covers_dir}/{title_id}.jpg`
  - [ ] Return local path string: `/covers/{title_id}.jpg`
  - [ ] `CoverError` enum: `Network(String)`, `InvalidImage(String)`, `Io(String)` — implement Display, all logged as `tracing::warn!`, never panic
  - [ ] Unit tests inline: valid JPEG resize, small image (≤400px) passthrough, invalid bytes, HTTP→HTTPS rewrite

- [x] Task 2: Covers Directory Configuration & ServeDir (AC: #3) — **PREREQUISITE FOR Task 3**
  - [ ] Add `covers_dir: PathBuf` to `AppState` in `src/lib.rs`
  - [ ] Read `COVERS_DIR` env var in `src/main.rs` (default: `"./covers"`): `std::env::var("COVERS_DIR").unwrap_or_else(|_| "./covers".to_string())`
  - [ ] Create directory at startup: `std::fs::create_dir_all(&covers_dir).expect("Failed to create covers directory")`
  - [ ] Pass `covers_dir` to AppState
  - [ ] Add `tower_http::services::ServeDir` route at `/covers` in `src/routes/mod.rs` (same pattern as existing `/static` ServeDir)
  - [ ] ServeDir needs the covers_dir path — pass it via `build_router()` parameter or from AppState

- [x] Task 3: Integrate Cover Download into Metadata Fetch (AC: #1, #7) — depends on Tasks 1 and 2
  - [ ] Update `fetch_metadata_chain()` signature in `src/tasks/metadata_fetch.rs`: add `covers_dir: PathBuf` parameter
  - [ ] Update ALL 4 callers in `src/routes/catalog.rs` (tokio::spawn fetch_metadata_chain calls) to pass `state.covers_dir.clone()` — search for "fetch_metadata_chain" to find all 4 call sites
  - [ ] After `update_title_from_metadata()` succeeds: if `metadata.cover_url` is `Some`, call `CoverService::download_and_resize()`
  - [ ] On success: run SQL `UPDATE titles SET cover_image_url = ? WHERE id = ? AND deleted_at IS NULL` with local path `/covers/{title_id}.jpg`
  - [ ] On failure: run SQL `UPDATE titles SET cover_image_url = NULL WHERE id = ? AND deleted_at IS NULL`, log `tracing::warn!(title_id, cover_url, error)`, continue to `mark_resolved()`
  - [ ] IMPORTANT: currently metadata_fetch writes remote cover_url to DB. Remove BOTH the SQL column `cover_image_url = COALESCE(?, cover_image_url),` from the UPDATE string AND the corresponding `.bind(&metadata.cover_url)` call — removing only one causes a bind count mismatch crash. After this task, cover_image_url is only set by the cover download step.

- [x] Task 4: Cover Component Template (AC: #4, #5)
  - [ ] Create `templates/components/cover.html` — Askama **macro** file (NOT include — Askama includes don't support parameters)
  - [ ] Define macro: `{% macro cover(cover_url, title, media_type, size_class, lazy) %}`
  - [ ] `size_class` is passed as a CSS class string (e.g., `"w-[200px] h-[300px]"`) — caller decides size
  - [ ] Missing state (cover_url is empty string ""): SVG icon from `/static/icons/{media_type}.svg` centered in `bg-stone-100 dark:bg-stone-800 rounded-lg {size_class}` container with `role="img"` and `aria-label="No cover available"`
  - [ ] Loaded state (cover_url is not empty): `<img src="{cover_url}" alt="Cover of {title}" class="{size_class} object-cover rounded-lg dark:shadow-sm" loading="{lazy}">` where lazy is "lazy" or "eager"
  - [ ] Shimmer/loading state: not in macro — handled inline in skeleton_feedback_html with `animate-pulse bg-stone-200 dark:bg-stone-700 rounded-lg`
  - [ ] NOTE: Askama macros use `{% call cover::cover(...) %}` syntax at call sites. Import via `{% import "components/cover.html" as cover %}`
  - [ ] NOTE: Askama macro parameters are all strings — pass `cover_url` as `&str` (empty string for None), not `Option`

- [x] Task 5: Template Integration (AC: #6) — depends on Task 4
  - [ ] Update `templates/pages/title_detail.html`:
    - Add `{% import "components/cover.html" as cover %}` at top
    - Replace inline `{% match title.cover_image_url %}` block with `{% call cover::cover(title.cover_image_url.as_deref().unwrap_or(""), title.title, title.media_type, "w-[200px] h-[300px]", "eager") %}`
  - [ ] Update `templates/pages/home.html`:
    - Add `{% import "components/cover.html" as cover %}` at top
    - Replace inline cover block with `{% call cover::cover(item.cover_image_url.as_deref().unwrap_or(""), item.title, item.media_type, "w-10 h-[60px]", "lazy") %}`
  - [ ] Both local paths (`/covers/42.jpg`) and remote URLs (`https://...`) work as `<img src>` — no special handling needed
  - [ ] NULL cover_image_url → empty string → Missing state with SVG placeholder

- [x] Task 6: E2E Tests & Mock Server (AC: #1, #4, #6)
  - [ ] Update `tests/e2e/mock-metadata-server/server.py`: add a route that serves a small test JPEG image (e.g., `GET /test-cover.jpg` returns a 100x150 red JPEG)
  - [ ] Update provider mock responses to include cover_url pointing to `http://mock-metadata:9090/test-cover.jpg`
  - [ ] Test ISBNs: use existing 9782070360246 (L'Etranger) — mock BnF response already returns this ISBN
  - [ ] Update `docker-compose.test.yml`: add `COVERS_DIR: /tmp/test-covers` env var + `tmpfs: /tmp/test-covers` for ephemeral test storage
  - [ ] E2E test: scan ISBN → wait for metadata resolve → verify `<img>` with `/covers/` src appears
  - [ ] E2E test: navigate to title detail → verify cover image visible
  - [ ] E2E test: title without cover → verify placeholder SVG

- [x] Task 7: i18n Keys
  - [ ] Add to `locales/en.yml`: `cover.alt_text: "Cover of %{title}"`, `cover.no_cover: "No cover available"`
  - [ ] Add to `locales/fr.yml`: `cover.alt_text: "Couverture de %{title}"`, `cover.no_cover: "Pas de couverture disponible"`
  - [ ] Run `touch src/lib.rs && cargo build`

## Dev Notes

### Architecture Compliance

- **Service layer:** Cover download logic in `src/services/cover.rs`, not in routes or tasks directly
- **Error handling:** `CoverError` enum, logged as `tracing::warn!`, never blocks scan workflow
- **Logging:** `tracing` macros with structured fields: `title_id`, `cover_url`, `error`, `file_size_bytes`
- **Static file serving:** tower-http `ServeDir` for `/covers/` path — same pattern as `/static/`
- **i18n:** Cover alt text via `t!("cover.alt_text", title = &title)`

### Critical Infrastructure Changes

**1. AppState needs `covers_dir` (Task 2):**
- Add `pub covers_dir: PathBuf` to `AppState` in `src/lib.rs`
- Initialize from `COVERS_DIR` env var in `main.rs`
- Pass to `fetch_metadata_chain()` via clone (PathBuf is cheap to clone)

**2. metadata_fetch.rs needs cover download step (Task 3):**
- Current flow: fetch metadata → update title (writes remote cover_url to DB) → mark resolved
- New flow: fetch metadata → update title (metadata fields only, NOT cover_image_url) → **download cover** → update cover_image_url with local path → mark resolved
- Cover download failure does NOT prevent mark_resolved — title metadata is still valid
- IMPORTANT: Remove the `.bind(&metadata.cover_url)` from the existing UPDATE query (line ~89) — cover_image_url is now only set by the cover download step

**3. cover_image_url meaning changes (Task 3):**
- Before 3-3: stores remote URL from provider (e.g., `https://covers.openlibrary.org/b/id/12345-L.jpg`)
- After 3-3: stores local path (e.g., `/covers/42.jpg`) or NULL if no cover
- Backward compatible: templates render both as `<img src="{url}">` — works for both HTTP URLs and local paths

### Existing Code to Reuse

**Templates already have cover display code:**
- `templates/pages/title_detail.html` lines 9-16: `{% match title.cover_image_url %}` with SVG fallback
- `templates/pages/home.html` lines 106-113: same pattern with thumbnail size
- These will be updated to use the new Cover component in Task 5

**SVG placeholder icons exist:**
- `/static/icons/book.svg`, `bd.svg`, `cd.svg`, `dvd.svg`, `magazine.svg`, `report.svg`
- Already used in existing templates

**Image crate in Cargo.toml:**
- `image = "0.25"` — already present, not yet used in source code
- Supports JPEG encoding, PNG decoding, resize with Lanczos3 filter

### Previous Story Intelligence (Story 3-2)

**Patterns to follow:**
- HTTPS URL rewriting: `url.replace("http://", "https://")`
- SSRF protection: validate URL components before HTTP calls
- Error handling: provider errors are `tracing::warn!`, never propagate as AppError
- Background task pattern: `tokio::spawn` with pool.clone()

**Review findings from 3-2 to avoid repeating:**
- Always write `cover_image_url` to DB in UPDATE query
- Validate URL paths to prevent SSRF
- Handle provider-specific error formats gracefully

### Image Processing Notes

```rust
use image::ImageReader;
use std::io::Cursor;

// Download
let bytes = client.get(url).send().await?.bytes().await?;

// Decode (auto-detect format: JPEG, PNG, GIF, WebP, etc.)
let img = ImageReader::new(Cursor::new(&bytes))
    .with_guessed_format()?
    .decode()?;

// Resize if needed (maintain aspect ratio)
let resized = if img.width() > 400 {
    img.resize(400, u32::MAX, image::imageops::FilterType::Lanczos3)
} else {
    img
};

// Encode as JPEG 80%
let mut output = Vec::new();
let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 80);
resized.write_with_encoder(encoder)?;

// Write to file
std::fs::write(format!("{covers_dir}/{title_id}.jpg"), &output)?;
```

### Deferred to Later Stories (NOT in scope)

- **Story 3-5:** Re-download metadata triggers new cover fetch; per-field confirmation includes cover
- Manual cover upload by user (not in any current story)
- Cover image garbage collection when title is deleted (soft-delete means cover persists)
- Animated image support (GIF/WebP → JPEG only)
- EXIF rotation handling (optional polish, not required)

### Project Structure Notes

New files to create:
- `src/services/cover.rs` — cover download + resize service
- `templates/components/cover.html` — Cover component template

Files to modify:
- `src/lib.rs` — add `covers_dir: PathBuf` to AppState
- `src/main.rs` — read COVERS_DIR, create directory, pass to AppState
- `src/routes/mod.rs` — add ServeDir for /covers/
- `src/tasks/metadata_fetch.rs` — integrate cover download after metadata update
- `src/services/mod.rs` — export cover module
- `templates/pages/title_detail.html` — use Cover component
- `templates/pages/home.html` — use Cover component
- `locales/en.yml`, `locales/fr.yml` — cover i18n keys
- `tests/e2e/docker-compose.test.yml` — COVERS_DIR volume
- `tests/e2e/mock-metadata-server/server.py` — serve test cover images

### References

- [Source: _bmad-output/planning-artifacts/epics.md#Epic-3] — FR14, FR15, FR88 requirements
- [Source: _bmad-output/planning-artifacts/architecture.md#Cover-Images] — AR11 ServeDir, storage path, resize specs
- [Source: _bmad-output/planning-artifacts/ux-design-specification.md#UX-DR10] — Cover component 3 states, 4 size variants
- [Source: _bmad-output/implementation-artifacts/3-2-media-type-scanning.md] — Previous story patterns, deferred cover work
- [Source: _bmad-output/implementation-artifacts/deferred-work.md] — Deferred items from 3-1 and 3-2

### Review Findings

- [x] [Review][Patch] Download size limit — FIXED: Content-Length check (max 10MB) + bytes.len() check before decode
- [x] [Review][Patch] Blocking I/O — FIXED: replaced std::fs::write with tokio::fs::write
- [x] [Review][Patch] Error logging — FIXED: log error when update_cover_image_url fails on cover download failure
- [x] [Review][Patch] i18n in cover.html — FIXED: added no_cover_label parameter to macro, passed from Rust via t!("cover.no_cover")
- [x] [Review][Defer] SSRF: no URL host validation on cover_url — deferred, URLs come from trusted metadata providers not user input
- [x] [Review][Defer] Race condition on concurrent cover write for same title_id — deferred, single-user NAS
- [x] [Review][Defer] No cache busting for re-downloaded covers — deferred, re-download is story 3-5
- [x] [Review][Defer] Optimistic locking missing on cover UPDATE — deferred, pre-existing pattern gap

## Dev Agent Record

### Agent Model Used

Claude Opus 4.6 (1M context)

### Debug Log References

### Completion Notes List

- Task 1: Created CoverService with download_and_resize() — image crate decode, Lanczos3 resize to 400px max, JPEG 80% encode, CoverError enum, 6 unit tests
- Task 2: Added covers_dir to AppState, COVERS_DIR env var with default, create_dir_all at startup, ServeDir at /covers/
- Task 3: Integrated cover download into metadata_fetch — added covers_dir + http_client params, cover download after metadata update, removed remote cover_url from UPDATE query, update_cover_image_url helper, updated all 4 callers in catalog.rs
- Task 4: Created Askama macro cover component — 2 states (loaded/missing), size_class parameter, lazy loading, dark mode shadow, SVG placeholder
- Task 5: Replaced inline cover code in title_detail.html and home.html with {% call cover::cover() %}{% endcall %} macro
- Task 6: Added test-cover.jpg route to mock server, updated Google Books thumbnail URL, COVERS_DIR in docker-compose, E2E test spec
- Task 7: Added cover.alt_text and cover.no_cover i18n keys in en/fr

### File List

New files:
- src/services/cover.rs
- templates/components/cover.html
- tests/e2e/specs/journeys/cover-image.spec.ts

Modified files:
- src/lib.rs (covers_dir in AppState)
- src/main.rs (COVERS_DIR env, create_dir_all, AppState init)
- src/services/mod.rs (export cover)
- src/routes/mod.rs (ServeDir /covers/)
- src/routes/catalog.rs (4 fetch_metadata_chain callers updated)
- src/tasks/metadata_fetch.rs (covers_dir + http_client params, cover download, removed cover_url bind)
- templates/pages/title_detail.html (cover macro)
- templates/pages/home.html (cover macro)
- locales/en.yml (cover keys)
- locales/fr.yml (cover keys)
- tests/e2e/mock-metadata-server/server.py (test-cover.jpg, thumbnail URL)
- tests/e2e/docker-compose.test.yml (COVERS_DIR)

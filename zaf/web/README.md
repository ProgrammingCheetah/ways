# The hub's page

A Svelte application that `zaf hub --port <n>` serves at `/`. Vite builds it into `dist/`, which
is committed and embedded in the zaf binary at compile time (`src/app/hub/dashboard/assets.rs`), so
`cargo build` and `cargo install` need no Node and zaf needs no web server. Node is only for
changing the page.

## Rebuild

```sh
cd zaf/web
npm ci          # first time, or when package-lock.json changes
npm run build   # writes dist/; commit it with the source
npm test        # Vitest: the page against a fixture hub
```

Commit `dist/` in the same change as the source it was built from. Its files have fixed names
(`assets/app.js`, `assets/app.css`, set in `vite.config.js`); a build that makes another file
fails `cargo test` (`every_built_file_is_served`) until `assets.rs` lists it.

After `npm install` changes a dependency, commit `package-lock.json` too.

## Develop

```sh
zaf hub --port 7777   # in a project, in another terminal
cd zaf/web && npm run dev
```

`npm run dev` proxies `/api` to the hub on port 7777.

## Layout

- `src/App.svelte`: the layout shell. A header holds the machine line; `main` is where the later
  views go, Next for you first and then one section for each project.
- `src/api.js`: `GET /api/state`, the change stream (`watchChanges` holds `GET /api/events` open,
  calls back on each `change` and opens a dropped stream again after a second) and the machine
  line's words. `App.svelte` reads the state again on each `change`.
- `src/controls.js`: the control key and `POST /api/<control>`. The page reads the key from the link's
  fragment (`#key=`), keeps it in memory for the tab only and strips it from the address; it
  travels in the `X-Zaf-Key` header. With `controls: false` in the state, or no key, no button shows.
- `src/Action.svelte`: one button that shows what happened, or the hub's reason for a refusal.
- `src/app.css`: light and dark from the system's theme; one column that works at phone width.

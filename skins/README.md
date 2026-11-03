# Skins

The skins the binaries embed (ADR 0097 §8; chapter 19 §5.10): **Plain**, which draws the
site's HTML and loads no stylesheet of its own, and **Worksheet**, the default, which is
Plain with Codex's and the site's stylesheets. An operator installs others as directories
under `server.skins` or `web.skins`, laid out like these: a `skin.toml` manifest,
`templates/` for a skin that stands alone, and the stylesheets and files a skin that
extends Plain adds. The loader is `crates/triplespace-ui/src/skin.rs`; the template
contract is `docs/api/skin-contract.md` once it leaves major 0.

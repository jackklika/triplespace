#!/usr/bin/env bash
# Records the parity corpus of the skin migration (19 §5.10): a test instance on the server's
# fixtures (the e2e entities, Librarybase's own wikitext pages, the internetdomains sample),
# seeded by `tools/skin_corpus.py` with a week's worth of activity, then every page the site
# serves recorded as three viewers into `crates/triplespace-ui/tests/corpus/exchanges/` with
# their cases in `cases.recorded.json`.
#
#   TRIPLESPACE_TEST_DATABASE_URL=postgres://… tools/skin_corpus.sh
#
# Needs the binaries built (`cargo build -p triplespace-cli -p triplespace-server
# -p triplespace-web`; BIN overrides target/debug), `ui/dist` built before them, and the
# `requests` Python package (PYTHON overrides python3; dev_env.sh's .venv has it).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
bin="${BIN:-$root/target/debug}"
python="${PYTHON:-python3}"
admin="${TRIPLESPACE_TEST_DATABASE_URL:?set TRIPLESPACE_TEST_DATABASE_URL}"
db="${DB:-tscorpus}"
url="${admin%/*}/$db"
port="${PORT:-18380}"
api="http://127.0.0.1:$port"
tenant=librarybase
fixtures="$root/crates/triplespace-server/tests/fixtures"
out="$root/crates/triplespace-ui/tests/corpus"
work="$(mktemp -d)"
pids=()
cleanup() {
	status=$?
	for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
	if [ "$status" -ne 0 ] && [ -f "$work/api.log" ]; then
		echo "--- API log"
		tail -n 30 "$work/api.log"
	fi
	psql "$admin" -q -c "DROP DATABASE IF EXISTS $db WITH (FORCE)" >/dev/null 2>&1 || true
	rm -rf "$work"
	exit "$status"
}
trap cleanup EXIT

psql "$admin" -q -c "DROP DATABASE IF EXISTS $db WITH (FORCE)" -c "CREATE DATABASE $db" >/dev/null
export TRIPLESPACE_DATABASE_URL="$url"
echo 'corpus-owner-secret-1234' >"$work/owner.pw"
"$bin/triplespace" instance create --farm-slug scatter --tenant $tenant --base https://librarybase.org \
	--key-file "$work/k.key" --provider internetdomains --adopt https://librarybase.org/ \
	--owner 2 --owner-name Alice --owner-password-file "$work/owner.pw" >/dev/null
# Registration is real but cheap (0087 §4), as tools/acl_check.sh sets it.
"$bin/triplespace" settings set --tenant $tenant challenge.bits 10 >/dev/null
"$bin/triplespace" settings set --tenant $tenant challenge.max_bits 14 >/dev/null
# The fixture's revisions are dated a day ago, to stay inside recent changes' 30-day window.
# Its author Alice is user 7 there, which Librarybase's wikitext slice gives to `Example`;
# an adoption refuses a source account the tenant holds under another name (0035 A16), so
# the owner is Alice under user 2, which neither dump uses.
yesterday="$(date -u -d '1 day ago' +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date -u -v-1d +%Y-%m-%dT%H:%M:%SZ)"
sed -e "s/2026-09-20T14:02:11Z/$yesterday/g" -e 's|<username>Alice</username><id>7</id>|<username>Alice</username><id>2</id>|g' \
	"$fixtures/site-entities.xml" >"$work/site-entities.xml"
"$bin/triplespace" adopt --tenant $tenant --dump "$work/site-entities.xml" \
	--source https://librarybase.org/ --version test --frozen --counters item=6,property=12 >/dev/null
"$bin/triplespace" adopt --tenant $tenant --dump "$fixtures/librarybase-wikitext.xml.gz" \
	--source https://librarybase.org/ --version pages --frozen >/dev/null
"$bin/triplespace" sync --provider internetdomains --dump "$fixtures/internetdomains.json" \
	--version 20261001 >/dev/null

"$bin/triplespace-server" --listen "127.0.0.1:$port" --key-file "$work/k.key" --mode development \
	--dev-tenant $tenant --trusted-proxy 127.0.0.1 --ui off >"$work/api.log" 2>&1 &
pids+=($!)
for _ in $(seq 100); do
	if curl -sf -o /dev/null "$api/w/api.php?action=query&meta=siteinfo&format=json"; then break; fi
	sleep 0.2
done

mkdir -p "$out/exchanges"
"$python" "$here/skin_corpus.py" --api "$api" --db "$url" --cli "$bin/triplespace" \
	--web "$bin/triplespace-web" --owner-password-file "$work/owner.pw" --out "$out"

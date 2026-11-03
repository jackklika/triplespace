#!/usr/bin/env bash
# Client compatibility (test plan §1.3–1.5): a database loaded from the Librarybase sample
# fixture, the API on its own, a bot account with a key, and then the three client checks —
# WikibaseIntegrator (api_check.py --wbi-login), Pywikibot (pwb_check.py) and
# WikidataIntegrator (wdi_check.py) — each as a hard pass/fail. First, before any client
# writes, every entity the fixture adopted is compared with the fixture itself
# (api_check.py --compare-dump): content, page ID, lastrevid and every revision's history.
#
#   TRIPLESPACE_TEST_DATABASE_URL=postgres://… tools/client_compat.sh
#
# Needs `cargo build -p triplespace-cli -p triplespace-server` (BIN overrides target/debug),
# psql, and a Python with the clients installed (PYTHON overrides python3):
#
#   python3 -m venv .venv && .venv/bin/pip install -r tools/requirements-compat.txt
#
# Everything the run creates — key file, passwords, logs, Pywikibot's family file and
# API cache — lives in a temporary directory removed on exit; nothing is written to the
# checkout. CHECKS limits the run (`CHECKS="wbi pwb"`); KEEP_DB=1 leaves the database.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
bin="${BIN:-$root/target/debug}"
python="${PYTHON:-python3}"
admin="${TRIPLESPACE_TEST_DATABASE_URL:?set TRIPLESPACE_TEST_DATABASE_URL}"
db="${DB:-tscompat}"
url="${admin%/*}/$db"
port="${PORT:-18280}"
api="http://127.0.0.1:$port"
tenant=librarybase
checks="${CHECKS:-dump wbi pwb wdi}"
fixture="${FIXTURE:-$root/crates/triplespace-server/tests/fixtures/librarybase-sample.xml.gz}"
work="$(mktemp -d)"
pids=()
cleanup() {
	status=$?
	for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
	if [ "$status" -ne 0 ] && [ -f "$work/api.log" ]; then
		echo "--- API log"
		tail -n 50 "$work/api.log"
	fi
	if [ -z "${KEEP_DB:-}" ]; then
		psql "$admin" -q -c "DROP DATABASE IF EXISTS $db WITH (FORCE)" >/dev/null 2>&1 || true
	fi
	rm -rf "$work"
	exit "$status"
}
trap cleanup EXIT

step() { printf '\n== %s\n' "$*"; }

step "database $db"
psql "$admin" -q -c "DROP DATABASE IF EXISTS $db WITH (FORCE)" -c "CREATE DATABASE $db" >/dev/null
export TRIPLESPACE_DATABASE_URL="$url"
echo 'compat-owner-secret-1234' >"$work/owner.pw"
"$bin/triplespace" instance create --farm-slug scatter --tenant $tenant --base https://librarybase.org \
	--key-file "$work/k.key" --provider internetdomains --adopt https://librarybase.org/ \
	--owner 7 --owner-name Alice --owner-password-file "$work/owner.pw" >/dev/null

step "adopt $(basename "$fixture")"
"$bin/triplespace" adopt --tenant $tenant --dump "$fixture" \
	--source https://librarybase.org/ --version sample --frozen 2>&1 | tail -n 8
"$bin/triplespace" sync --provider internetdomains \
	--dump "$root/crates/triplespace-server/tests/fixtures/internetdomains.json" \
	--version 20261001 >/dev/null

step "bot account"
"$bin/triplespace" subsidiary create --tenant $tenant --name CompatBot --operator Alice --group bot >/dev/null
"$bin/triplespace" subsidiary key --tenant $tenant --name CompatBot --label ci \
	--grant editentity --grant createeditmovepage --grant highvolume --json >"$work/key.json"
login="$($python -c 'import json,sys; print(json.load(open(sys.argv[1]))["login"])' "$work/key.json")"
TRIPLESPACE_WBI_SECRET="$($python -c 'import json,sys; print(json.load(open(sys.argv[1]))["secret"])' "$work/key.json")"
export TRIPLESPACE_WBI_SECRET
item_floor="$(psql "$url" -Atc "SELECT last_value FROM log.\"$tenant.item_id\"")"
# The item the checks edit: the lowest-numbered one the fixture holds, unless ITEM says.
item="${ITEM:-$(psql "$url" -Atc "SELECT id FROM view.entity WHERE tenant = '$tenant' AND type = 'item' ORDER BY length(id), id LIMIT 1")}"
echo "login $login; item $item; item floor $item_floor"

step "server on $api"
"$bin/triplespace-server" --listen "127.0.0.1:$port" --key-file "$work/k.key" --mode development \
	--dev-tenant $tenant --trusted-proxy 127.0.0.1 --ui off >"$work/api.log" 2>&1 &
pids+=($!)
for _ in $(seq 100); do
	if curl -sf -o /dev/null "$api/w/api.php?action=query&meta=siteinfo&format=json"; then break; fi
	sleep 0.2
done
curl -sf -o /dev/null "$api/w/api.php?action=query&meta=siteinfo&format=json" || {
	echo "server did not come up"
	exit 1
}

failed=()
run() {
	local name=$1
	shift
	step "$name"
	if "$@"; then echo "-- $name: passed"; else
		echo "-- $name: FAILED ($?)"
		failed+=("$name")
	fi
}

# Pywikibot and WDI keep per-user state (API cache, family files, login cookies); point all
# of it into the temporary directory so a run never touches $HOME.
export HOME="$work/home"
mkdir -p "$HOME"
export PYWIKIBOT_NO_USER_CONFIG=1

for c in $checks; do
	case $c in
	dump) run "Adopted as the fixture has it (api_check.py --compare-dump)" "$python" "$here/api_check.py" \
		--api "$api" --tenant $tenant --db "$url" --sample "$(psql "$url" -Atc "SELECT count(*) FROM view.entity WHERE tenant = '$tenant'")" \
		--compare-dump "$fixture" ;;
	wbi) run "WikibaseIntegrator (api_check.py)" "$python" "$here/api_check.py" --api "$api" --tenant $tenant \
		--db "$url" --ids "$item" --sample 10 --wbi-login "$login" --wbi-create-property ;;
	pwb) run "Pywikibot (pwb_check.py)" "$python" "$here/pwb_check.py" --api "$api" --tenant $tenant \
		--login "$login" --item "$item" --item-floor "$item_floor" ;;
	wdi) run "WikidataIntegrator (wdi_check.py)" "$python" "$here/wdi_check.py" --api "$api" \
		--login "$login" --item "$item" --item-floor "$item_floor" ;;
	*)
		echo "unknown check $c (dump, wbi, pwb, wdi)"
		exit 2
		;;
	esac
done

step "summary"
if [ ${#failed[@]} -gt 0 ]; then
	echo "failed: ${failed[*]}"
	exit 1
fi
echo "all client checks passed"

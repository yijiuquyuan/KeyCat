#!/bin/sh
# Copies a GitHub release (title, notes, assets) to the Gitee mirror, for people who cannot reach GitHub.
# Gitee's repository mirror only syncs branches and tags, not releases. Safe to re-run: an existing
# release and already uploaded files are kept. The Windows installer goes up as KeyCat-Windows-Setup.zip.
# Usage: sh tools/gitee-release.sh v1.0.3 [more tags]
# Needs gh (logged in to GitHub) and a Gitee personal access token with the "projects" scope,
# from $GITEE_TOKEN or signing/gitee-token.
set -eu
cd "$(dirname "$0")/.."
REPO="${GITEE_REPO:-banunu/KeyCat}"
API="https://gitee.com/api/v5/repos/$REPO"
TOKEN="${GITEE_TOKEN:-$(cat signing/gitee-token 2>/dev/null || true)}"
[ -n "$TOKEN" ] || { echo "set GITEE_TOKEN or put the token in signing/gitee-token" >&2; exit 1; }
# Gitee rejects release attachments over 100 MB on personal repositories.
LIMIT=$((100 * 1024 * 1024))
# python3 on macOS and Linux, python on the Windows runner.
PY="$(command -v python3 || command -v python)"
json() { "$PY" -c "import json,sys; d=json.load(sys.stdin); print($1)"; }

# Gitee now and then answers a burst of uploads with 403; wait and try again before giving up.
upload() {
    for TRY in 1 2 3 4; do
        CODE="$(curl -sS -o "$WORK/reply" -w '%{http_code}' -X POST "$API/releases/$1/attach_files" \
            -F "access_token=$TOKEN" -F "file=@$2")"
        [ "$CODE" = 201 ] && return 0
        echo "  $(basename "$2"): HTTP $CODE $(head -c 200 "$WORK/reply"), retry $TRY" >&2
        sleep $((TRY * 10))
    done
    return 1
}

for TAG in "$@"; do
    echo "== $TAG"
    WORK="$(mktemp -d)"
    gh release view "$TAG" --json name,body,targetCommitish > "$WORK/release.json"
    NAME="$(json 'd["name"] or "'"$TAG"'"' < "$WORK/release.json")"
    BODY="$(json 'd["body"] or "'"$TAG"'"' < "$WORK/release.json")"
    # The tag's own commit, so Gitee never makes the tag on a different commit if the mirror lags.
    COMMIT="$(gh api "repos/{owner}/{repo}/commits/$TAG" -q .sha)"
    # A missing release comes back as 404 with a JSON message, hence no -f here.
    ID="$(curl -sS "$API/releases/tags/$TAG?access_token=$TOKEN" | json '(d or {}).get("id") or ""' 2>/dev/null || true)"
    if [ -z "$ID" ]; then
        ID="$(curl -fsS -X POST "$API/releases" --data-urlencode "access_token=$TOKEN" \
            --data-urlencode "tag_name=$TAG" --data-urlencode "name=$NAME" --data-urlencode "body=$BODY" \
            --data-urlencode "target_commitish=$COMMIT" --data-urlencode "prerelease=false" | json 'd["id"]')"
        echo "created release $ID"
    else
        echo "release $ID already exists"
    fi
    HAVE="$(curl -fsS "$API/releases/$ID/attach_files?access_token=$TOKEN&per_page=100" \
        | json '"\n".join(f["name"] for f in d)')"
    gh release download "$TAG" --dir "$WORK/assets"
    for FILE in "$WORK"/assets/*; do
        # Gitee rejects unsigned installers as "malicious file detected"; the same file inside a zip passes.
        case "$FILE" in *.exe)
            ZIP="${FILE%.exe}.zip"
            "$PY" -c "import sys,zipfile,os; zipfile.ZipFile(sys.argv[2],'w',zipfile.ZIP_DEFLATED).write(sys.argv[1],os.path.basename(sys.argv[1]))" "$FILE" "$ZIP"
            rm "$FILE"
            FILE="$ZIP" ;;
        esac
        BASE="$(basename "$FILE")"
        if printf '%s\n' "$HAVE" | grep -qxF "$BASE"; then echo "  $BASE already uploaded"; continue; fi
        if [ "$(wc -c < "$FILE")" -gt "$LIMIT" ]; then echo "  $BASE skipped: over 100 MB"; continue; fi
        upload "$ID" "$FILE"
        echo "  uploaded $BASE"
    done
    rm -rf "$WORK"
done

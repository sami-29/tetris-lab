#!/usr/bin/env bash
# Point the probe at a custom subdomain, or undo it (ticket 09).
#   ./host-probe/apply-route.sh lab.example.com   # uncomment + set the route
#   ./host-probe/apply-route.sh --clear           # back to workers.dev only
# Requires that the domain's zone is active on the same Cloudflare account; a
# route without a zone fails the deploy. Re-running rewrites the same line, so
# the config never accumulates routes.
set -euo pipefail
cd "$(dirname "$0")"

if [[ "${1:-}" == "--clear" ]]; then
  replacement='  // ROUTE: "routes": [{ "pattern": "lab.example.com", "custom_domain": true }]'
  action="cleared the custom-domain route"
else
  domain="${1:-}"
  if [[ -z "$domain" ]]; then
    echo "usage: $0 <hostname>|--clear   (e.g. lab.example.com)" >&2
    exit 2
  fi
  if [[ "$domain" != *.* || "$domain" == */* || "$domain" == *" "* ]]; then
    echo "not a hostname: $domain" >&2
    exit 2
  fi
  replacement="  \"routes\": [{ \"pattern\": \"$domain\", \"custom_domain\": true }]"
  action="routed $domain"
fi

tmp=$(mktemp)
awk -v repl="$replacement" '
  /^ *\/\/ ROUTE:/ || /^ *"routes":/ { print repl; next }
  { print }
' wrangler.jsonc >"$tmp"
mv "$tmp" wrangler.jsonc

echo "wrangler.jsonc: $action"
grep -n 'ROUTE:\|"routes":' wrangler.jsonc
echo
echo "next: npx wrangler deploy   then   ./verify.sh https://<hostname>"

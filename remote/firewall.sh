#!/usr/bin/env bash
# Egress allowlist via nftables. Runs as root at boot.
set -euo pipefail

# Allowed FQDNs (resolved to A records, pinned for this boot).
ALLOWED=(
  api.anthropic.com
  claude.ai
  console.anthropic.com
  github.com
  api.github.com
  codeload.github.com
  objects.githubusercontent.com
  raw.githubusercontent.com
  ghcr.io
  pkg-containers.githubusercontent.com
  registry.npmjs.org
  registry.yarnpkg.com
  pub.dev
  storage.googleapis.com
  storage.flutter-io.cn
  dart.dev
  deb.nodesource.com
  cli.github.com
  deb.debian.org
  security.debian.org
  archive.ubuntu.com
  security.ubuntu.com
  ports.ubuntu.com
)

IPS=()
for d in "${ALLOWED[@]}"; do
  while read -r ip; do
    [ -n "$ip" ] && IPS+=("$ip")
  done < <(getent ahostsv4 "$d" | awk '{print $1}' | sort -u)
done

nft flush ruleset || true

nft -f - <<EOF
table inet fw {
  set allowed4 {
    type ipv4_addr
    flags interval
    elements = { $(IFS=, ; echo "${IPS[*]}") }
  }
  chain input {
    type filter hook input priority 0; policy accept;
  }
  chain output {
    type filter hook output priority 0; policy drop;
    ct state established,related accept
    oifname "lo" accept
    ip daddr 127.0.0.0/8 accept
    ip daddr 10.0.0.0/8 accept
    ip daddr 172.16.0.0/12 accept
    ip daddr 192.168.0.0/16 accept
    ip daddr fd00::/8 accept
    udp dport 53 accept
    tcp dport 53 accept
    udp dport 123 accept
    ip daddr @allowed4 tcp dport { 443, 80 } accept
    counter drop
  }
  chain forward {
    type filter hook forward priority 0; policy drop;
  }
}
EOF

echo "firewall applied: ${#IPS[@]} allowlisted IPs"

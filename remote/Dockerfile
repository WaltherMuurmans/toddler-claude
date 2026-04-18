FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive
ENV PATH="/home/dev/.local/bin:/home/dev/flutter/bin:/home/dev/flutter/bin/cache/dart-sdk/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates curl git gnupg lsb-release sudo unzip xz-utils jq \
      build-essential pkg-config \
      nftables iproute2 dnsutils iputils-ping \
      postgresql-client \
      python3 python3-pip \
      tini \
      locales \
    && locale-gen en_US.UTF-8 \
    && rm -rf /var/lib/apt/lists/*

# Node 20 via NodeSource
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
 && apt-get install -y nodejs \
 && rm -rf /var/lib/apt/lists/*

# GitHub CLI
RUN curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
      | dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg \
 && chmod go+r /usr/share/keyrings/githubcli-archive-keyring.gpg \
 && echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
      > /etc/apt/sources.list.d/github-cli.list \
 && apt-get update && apt-get install -y gh && rm -rf /var/lib/apt/lists/*

# ttyd
RUN curl -fsSL -o /usr/local/bin/ttyd \
      https://github.com/tsl0922/ttyd/releases/download/1.7.7/ttyd.x86_64 \
 && chmod +x /usr/local/bin/ttyd

# Non-root user
RUN useradd -m -s /bin/bash -G sudo dev \
 && echo 'dev ALL=(ALL) NOPASSWD: /usr/sbin/nft, /usr/bin/apt-get' >> /etc/sudoers

# Postgres (embedded, data in tmpfs at runtime)
RUN apt-get update && apt-get install -y --no-install-recommends postgresql-16 \
 && rm -rf /var/lib/apt/lists/*

USER dev
WORKDIR /home/dev

# Flutter stable
RUN curl -fsSL -o /tmp/flutter.tar.xz \
      https://storage.googleapis.com/flutter_infra_release/releases/stable/linux/flutter_linux_3.24.5-stable.tar.xz \
 && tar -xJf /tmp/flutter.tar.xz -C /home/dev \
 && rm /tmp/flutter.tar.xz \
 && flutter --version \
 && flutter config --no-analytics \
 && flutter precache --no-android --no-ios --no-web --no-windows --no-macos --no-linux-desktop --no-universal || true

# Claude Code CLI (installs to ~/.local/bin/claude)
RUN curl -fsSL https://claude.ai/install.sh | bash || \
    npm config set prefix "$HOME/.local" && npm install -g @anthropic-ai/claude-code

USER root
COPY boot.sh /opt/boot.sh
COPY firewall.sh /opt/firewall.sh
COPY welcome.txt /opt/welcome.txt
COPY pg-init.sh /opt/pg-init.sh
COPY claude-wrapper.sh /opt/claude-wrapper.sh
RUN chmod +x /opt/boot.sh /opt/firewall.sh /opt/pg-init.sh /opt/claude-wrapper.sh

EXPOSE 7681

ENTRYPOINT ["/usr/bin/tini", "--", "/opt/boot.sh"]

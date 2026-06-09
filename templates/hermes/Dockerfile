FROM node:20-bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
      python3 python3-pip \
      curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Install hermes-agent CLI. uv is the fastest path on Python 3.x.
RUN pip install --no-cache-dir --break-system-packages uv \
 && uv pip install --system --no-cache hermes-agent

WORKDIR /app

COPY package.json ./
RUN npm install --omit=dev

COPY src ./src

ENV PORT=8080 \
    LITELLM_DEFAULT_MODEL=claude-sonnet-4-6

EXPOSE 8080

CMD ["node", "src/index.mjs"]

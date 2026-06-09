FROM node:20-bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
      python3 python3-pip \
      curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN pip install --no-cache-dir --break-system-packages uv \
 && uv pip install --system --no-cache hermes-agent

WORKDIR /app

COPY package.json ./
RUN npm install --omit=dev --no-audit --no-fund

COPY src ./src

ENV PORT=8080 \
    LITELLM_DEFAULT_MODEL=claude-sonnet-4-6

EXPOSE 8080

CMD ["node", "src/index.mjs"]

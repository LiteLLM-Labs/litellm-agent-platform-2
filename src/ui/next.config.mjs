import { PHASE_DEVELOPMENT_SERVER } from "next/constants.js";

const devApiBase = (
  process.env.LITELLM_DEV_PROXY_TARGET ??
  process.env.LITELLM_DEV_API_BASE ??
  "http://127.0.0.1:4000"
).replace(/\/+$/, "");

const sharedConfig = {
  images: { unoptimized: true },
};

const exportConfig = {
  ...sharedConfig,
  output: "export",
  trailingSlash: true,
};

const devConfig = {
  ...sharedConfig,
  allowedDevOrigins: ["127.0.0.1"],
  trailingSlash: false,
  async rewrites() {
    return [
      { source: "/api/:path*", destination: `${devApiBase}/api/:path*` },
      { source: "/v1/:path*", destination: `${devApiBase}/v1/:path*` },
      { source: "/session", destination: `${devApiBase}/session` },
      { source: "/session/:path*", destination: `${devApiBase}/session/:path*` },
      { source: "/event", destination: `${devApiBase}/event` },
      { source: "/mcp", destination: `${devApiBase}/mcp` },
      { source: "/mcp/:path*", destination: `${devApiBase}/mcp/:path*` },
      { source: "/whoami", destination: `${devApiBase}/whoami` },
    ];
  },
};

export default function nextConfig(phase) {
  return phase === PHASE_DEVELOPMENT_SERVER ? devConfig : exportConfig;
}

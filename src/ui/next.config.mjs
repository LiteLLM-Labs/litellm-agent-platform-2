import { PHASE_DEVELOPMENT_SERVER } from "next/constants.js";

export default function nextConfig(phase) {
  const apiBase = process.env.LITELLM_DEV_API_BASE?.replace(/\/+$/, "");
  const isDev = phase === PHASE_DEVELOPMENT_SERVER;
  return {
    output: isDev ? undefined : "export",
    trailingSlash: true,
    images: { unoptimized: true },
    allowedDevOrigins: ["127.0.0.1"],
    ...(isDev && apiBase
      ? {
          async rewrites() {
            return [
              { source: "/api/:path*", destination: `${apiBase}/api/:path*` },
              { source: "/v1/:path*", destination: `${apiBase}/v1/:path*` },
              { source: "/session/:path*", destination: `${apiBase}/session/:path*` },
              { source: "/event", destination: `${apiBase}/event` },
              { source: "/whoami", destination: `${apiBase}/whoami` },
            ];
          },
        }
      : {}),
  };
}

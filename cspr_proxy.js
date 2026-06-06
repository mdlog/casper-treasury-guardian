/**
 * CSPR.cloud authentication proxy.
 *
 * Odra's livenet env cannot attach a custom Authorization header, but CSPR.cloud's
 * node RPC requires one. This tiny proxy listens locally and forwards every
 * request to CSPR.cloud with the token from .env attached.
 *
 * Usage:
 *   1. Put CSPR_CLOUD_AUTH_TOKEN in .env (see .env.example).
 *   2. node cspr_proxy.js
 *   3. Point ODRA_CASPER_LIVENET_NODE_ADDRESS at http://127.0.0.1:7778/rpc
 *   4. Deploy / interact: cargo run --bin deploy_testnet --features=livenet
 */
const http = require("http");
const https = require("https");
const fs = require("fs");
const path = require("path");

function loadEnv() {
  try {
    const envContent = fs.readFileSync(path.join(__dirname, ".env"), "utf8");
    envContent.split("\n").forEach((line) => {
      const trimmed = line.trim();
      if (trimmed && !trimmed.startsWith("#")) {
        const [key, ...values] = trimmed.split("=");
        if (key && values.length > 0) {
          process.env[key.trim()] = values.join("=").trim();
        }
      }
    });
  } catch {
    console.error("Error: could not read .env file (copy .env.example to .env)");
    process.exit(1);
  }
}

loadEnv();

const CSPR_CLOUD_TOKEN = process.env.CSPR_CLOUD_AUTH_TOKEN;
const UPSTREAM_HOST = process.env.CSPR_CLOUD_HOST ?? "node.testnet.cspr.cloud";
const LOCAL_PORT = Number(process.env.CSPR_PROXY_PORT ?? 7778);

if (!CSPR_CLOUD_TOKEN) {
  console.error("Error: CSPR_CLOUD_AUTH_TOKEN not found in .env");
  process.exit(1);
}

const server = http.createServer((req, res) => {
  let body = "";
  req.on("data", (chunk) => (body += chunk.toString()));
  req.on("end", () => {
    const proxyReq = https.request(
      {
        hostname: UPSTREAM_HOST,
        port: 443,
        path: req.url,
        method: req.method,
        headers: {
          "Content-Type": "application/json",
          Authorization: CSPR_CLOUD_TOKEN,
          "Content-Length": Buffer.byteLength(body),
        },
      },
      (proxyRes) => {
        res.writeHead(proxyRes.statusCode, proxyRes.headers);
        proxyRes.on("data", (chunk) => res.write(chunk));
        proxyRes.on("end", () => res.end());
      }
    );
    proxyReq.on("error", (error) => {
      console.error("Proxy error:", error.message);
      res.writeHead(500, { "Content-Type": "application/json" });
      res.end(JSON.stringify({ error: "Proxy error", details: error.message }));
    });
    if (body) proxyReq.write(body);
    proxyReq.end();
  });
});

server.listen(LOCAL_PORT, "127.0.0.1", () => {
  console.log(`CSPR.cloud proxy running on http://127.0.0.1:${LOCAL_PORT}`);
  console.log(`Forwarding to https://${UPSTREAM_HOST}`);
  console.log("Set ODRA_CASPER_LIVENET_NODE_ADDRESS=http://127.0.0.1:" + LOCAL_PORT + "/rpc");
});

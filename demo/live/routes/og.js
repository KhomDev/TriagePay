import { fail } from "../lib/http.js";
import { renderReceiptPng, renderSitePng, sanitizeReceiptQuery } from "../lib/og-image.js";

const IMAGE_HEADERS = {
  "content-type": "image/png",
  "cache-control": "public, max-age=300",
  "access-control-allow-origin": "*",
};

function imageResponse(png) {
  const body = Buffer.from(png);
  return new Response(body, { status: 200, headers: { ...IMAGE_HEADERS, "content-length": String(body.length) } });
}

async function siteCard() {
  return imageResponse(await renderSitePng());
}

export async function GET(request) {
  const params = new URL(request.url).searchParams;
  for (const key of ["route", "path"]) params.delete(key);
  if (!params.get("kind")) {
    try {
      return await siteCard();
    } catch {
      return fail(500, "og_failed", "The receipt image could not be rendered.");
    }
  }
  const fields = sanitizeReceiptQuery(params);
  if (!fields) return fail(400, "bad_receipt", "The receipt fields were rejected.");
  try {
    return imageResponse(await renderReceiptPng(fields));
  } catch {
    try {
      return await siteCard();
    } catch {
      return fail(500, "og_failed", "The receipt image could not be rendered.");
    }
  }
}

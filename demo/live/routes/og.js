import { fail } from "../lib/http.js";
import { renderReceiptPng, renderSitePng, sanitizeReceiptQuery } from "../lib/og-image.js";

export async function GET(request) {
  const params = new URL(request.url).searchParams;
  if ([...params.keys()].length === 0) {
    const png = await renderSitePng();
    return new Response(png, {
      status: 200,
      headers: { "content-type": "image/png", "cache-control": "public, max-age=300" },
    });
  }
  const fields = sanitizeReceiptQuery(params);
  if (!fields) return fail(400, "bad_receipt", "The receipt fields were rejected.");
  try {
    const png = await renderReceiptPng(fields);
    return new Response(png, {
      status: 200,
      headers: {
        "content-type": "image/png",
        "cache-control": "public, max-age=300",
      },
    });
  } catch (error) {
    return fail(500, "og_failed", "The receipt image could not be rendered.");
  }
}

import { renderSitePng } from "../lib/og-image.js";
import { fail } from "../lib/http.js";

export async function GET() {
  try {
    const png = await renderSitePng();
    return new Response(png, {
      status: 200,
      headers: { "content-type": "image/png", "cache-control": "public, max-age=3600" },
    });
  } catch {
    return fail(500, "og_failed", "The site image could not be rendered.");
  }
}

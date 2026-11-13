import { json } from "../lib/http.js";
import { boardPayload, HOME_CACHE, homeResponse } from "../lib/live-board.js";

export function GET() {
  return homeResponse();
}

export async function boardGet() {
  const board = await boardPayload();
  return json(200, board, { "cache-control": HOME_CACHE });
}

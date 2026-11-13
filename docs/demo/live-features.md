# Hosted demo features

I added these on the judge page in `demo/live`. None of them put a key in the browser. Names only here. Values stay out of git.

## Drain Me

The form posts to `/api/drain`. I keep the text under 500 characters, drop null bytes, and store a redacted 180-character line. The redaction covers an `ed25519:` secret and a 64-character hex string.

The scripted agent pays the first NEAR account in the text, at the demo amount. A live model, when `NEARAI_API_KEY` is set, may name a different account. I read the wallet and the review policy before I sign. If that payment would settle, I do not sign, and the feed says `not_signed`. A refusal or a hold is a real transaction. The drained counter adds an amount only when the chain outcome is a settle.

The per-address cap is `TRIAGEPAY_DRAIN_IP_LIMIT` (default 3) per `TRIAGEPAY_DRAIN_WINDOW_SECONDS` (default 600). With `KV_REST_API_URL` and `KV_REST_API_TOKEN`, or the Upstash pair, the cap and the feed live in that store. If the store is set and does not answer, I do not run the attempt. If it is unset, the cap and the feed are memory in this process. The journal file still records a signed attempt for the recent-activity table. The daily signing cap is unchanged: it still comes from the shared store or the chain index.

## The model

`NEARAI_API_KEY` turns on a chat completion at `https://cloud-api.near.ai/v1/chat/completions`. `NEARAI_MODEL` picks the model. I default to `z-ai/glm-5.3-flash`. The finding is the user text. The only tool is `pay`. I send `x-no-aliasing: true`. The page asks for `text/event-stream` and shows the reasoning, then the tool call, then the wallet result. A missing key, an HTTP error, or a timeout falls back to the scripted agent and the label stays `scripted`. I do not put the key in an error string.

## Receipts

`/api/og` returns a PNG. `/api/share` returns a small HTML page with the image as `og:image`, so a post can unfurl. Query fields are checked against a tight pattern. A payee has to be a NEAR account. The share text is `built at NEAR HACKATHON @NEARProtocol #NEARHackathon`. The image is drawn with Satori and resvg, the same pair an OG route uses.

## Telegram

1. In BotFather, send `/newbot` and copy the token into `TELEGRAM_BOT_TOKEN`.
2. Open the bot, send it any message, and read the chat id. Put that in `TELEGRAM_OWNER_CHAT_ID`. For a private chat it is the user id.
3. Pick a long random `TELEGRAM_WEBHOOK_SECRET` (letters, digits, `_`, `-`).
4. Register the webhook. Replace the host and the two secrets. Do not commit the command after you fill it in.

```sh
curl -s "https://api.telegram.org/bot${TELEGRAM_BOT_TOKEN}/setWebhook" \
  -d "url=https://triagepay-demo.vercel.app/api/telegram" \
  -d "secret_token=${TELEGRAM_WEBHOOK_SECRET}" \
  -d "allowed_updates[]=callback_query"
```

The route rejects a missing or wrong `X-Telegram-Bot-Api-Secret-Token`. It also rejects a callback whose chat id is not `TELEGRAM_OWNER_CHAT_ID`. Approve sends `a:<payment id>` and Reject sends `r:<payment id>`. Both call the existing owner path. If the variables are unset, I skip the message and the page says so.

## Kill switch

`KILL_SWITCH_SECRET` is the HMAC key. I mint `expiry.nonce.mac`, show the URL and a QR code, and expire it after `KILL_SWITCH_TTL_SECONDS` (default 600, clamped between 30 and 3600). Opening `/api/kill?t=...` checks the mac and the expiry, marks the nonce used, then revokes. If the revoke fails, I drop the nonce so the link can be opened again. With a shared store the nonce is global. Without one, only this process remembers it, and the card says so.

The link is as public as the Revoke button once it is on the page. It is single-use and it expires. I do not mint one when the secret is unset.

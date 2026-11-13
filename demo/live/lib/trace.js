import { honeypotAccount, recordHoneypot } from "./honeypot.js";
import { RULE_TEXT } from "./receipt.js";
import { saveReplay } from "./replay.js";

export async function traceOutcome(config, input) {
  try {
    const code = input.errorCode || (input.held ? "HELD_FOR_OWNER" : null);
    const interesting = input.held || input.errorCode || input.outcome !== "success";
    if (input.txHash && interesting) {
      await saveReplay({
        txHash: input.txHash,
        injection: input.injection,
        reasoning: input.reasoning,
        agentMode: input.agentMode,
        payee: input.payee,
        amountDisplay: input.amountDisplay,
        errorCode: input.errorCode,
        held: Boolean(input.held),
        outcome: input.outcome,
        ruleText: (code && RULE_TEXT[code]) || input.ruleText || null,
        explorerUrl: input.explorerUrl,
        checks: input.checks || [],
      });
    }
    const trap = honeypotAccount(config);
    if (trap && input.payee && String(input.payee).toLowerCase() === trap && input.txHash && input.outcome !== "success") {
      await recordHoneypot({
        payee: String(input.payee).toLowerCase(),
        outcome: input.held ? "held" : "refused",
        errorCode: input.errorCode,
        txHash: input.txHash,
        explorerUrl: input.explorerUrl,
        prompt: input.injection,
      });
    }
  } catch {
    // A missing record must not hide the chain result.
  }
}

import { ReplayRenderer, type BatchView } from "wows-toolkit/native";
import { errorMessage } from "../components";
import * as toast from "./toast";

const BATCH_PROGRESS = "batch-render";

let followingBatch = false;

/** Says the batch render as the other front ends do: a progress toast while it runs, then how it ended. */
function sayBatch(view: BatchView) {
  if (view.running) {
    if (view.text) toast.progress(BATCH_PROGRESS, view.text);
    return;
  }
  toast.resolved(BATCH_PROGRESS);
  if (!view.text) return;
  if (view.failed) toast.warn(view.text);
  else toast.ok(view.text);
}

async function followBatch() {
  if (followingBatch) return;
  followingBatch = true;
  // What was there before this follower is not news; only a batch that
  // stops running after it publishes `running: false` again.
  let revision = ReplayRenderer.batch().revision;
  for (;;) {
    const next = await ReplayRenderer.nextBatchChange(revision);
    revision = next.revision;
    sayBatch(next);
  }
}

/**
 * Renders each replay in `paths` to its own video, into a folder the reader
 * picks or onto the clipboard. `null` once started; the reason when it could
 * not start.
 */
export async function renderReplaysToVideo(paths: string[], toClipboard: boolean): Promise<string | null> {
  void followBatch();
  try {
    await ReplayRenderer.renderMany(paths, toClipboard);
    return null;
  } catch (error) {
    return errorMessage(error);
  }
}

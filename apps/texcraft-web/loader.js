// The loading screen's progress while the browser downloads the editor (Trunk's initializer hooks,
// `data-initializer` in index.html).
//
// This is TeXCraft's only handwritten JavaScript, a departure from AGENTS.md rule 11 decided by
// the operator (2026-11-22): before the WebAssembly module has arrived there is no Rust to run, so
// only a script can follow the download. It reads progress and draws nothing but the page's own
// loading screen; the app removes that screen once it draws its first frame.

function megabytes(bytes) {
  return (bytes / 1e6).toFixed(1);
}

export default function loader() {
  const screen = document.getElementById("loading");
  const bar = screen && screen.querySelector(".bar");
  const label = screen && screen.querySelector(".status");
  return {
    onStart: () => {},
    // `total` is 0 when the server does not say the size (a compressed response): the bar then
    // keeps moving without a percentage, and the label counts what has arrived.
    onProgress: ({ current, total }) => {
      if (!bar || !label) return;
      if (total > 0) {
        const share = Math.min(1, current / total);
        bar.classList.add("known");
        bar.style.setProperty("--done", `${(share * 100).toFixed(1)}%`);
        bar.setAttribute("aria-valuenow", String(Math.round(share * 100)));
        label.textContent = `Loading the editor… ${megabytes(current)} of ${megabytes(total)} MB`;
      } else {
        label.textContent = `Loading the editor… ${megabytes(current)} MB`;
      }
    },
    onComplete: () => {
      if (label) label.textContent = "Starting the editor…";
      if (bar) bar.style.setProperty("--done", "100%");
    },
    onSuccess: () => {},
    onFailure: (error) => {
      if (label) label.textContent = "The editor could not be loaded. Reload the page to try again.";
      console.error(error);
    },
  };
}

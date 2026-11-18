// Outside links (Cloudflare, guides, mail) always open in the normal browser.
// Instead of relying on how each desktop web view treats target="_blank" (WebView2 on
// Windows, WebKit on Mac), every click on an outside link is sent to Rust's open_url.
import { invoke } from "@tauri-apps/api/core";

const isOutside = (href: string) => {
  try {
    const url = new URL(href, window.location.href);
    return (url.protocol === "http:" || url.protocol === "https:" || url.protocol === "mailto:") && url.origin !== window.location.origin;
  } catch {
    return false;
  }
};

export const openOutside = (href: string) =>
  invoke("open_url", { url: href }).catch((e) => console.error("Could not open link", e));

export function routeOutsideLinks() {
  document.addEventListener(
    "click",
    (event) => {
      if (event.defaultPrevented || event.button !== 0) return;
      const link = (event.target as Element | null)?.closest?.("a[href]") as HTMLAnchorElement | null;
      if (!link || !isOutside(link.href)) return;
      event.preventDefault();
      void openOutside(link.href);
    },
    true,
  );
  const open = window.open.bind(window);
  window.open = (url?: string | URL, target?: string, features?: string) => {
    const href = url?.toString() ?? "";
    if (href && isOutside(href)) {
      void openOutside(new URL(href, window.location.href).href);
      return null;
    }
    return open(url, target, features);
  };
}

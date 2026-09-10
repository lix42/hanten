import { HeadContent, Outlet, Scripts, createRootRoute } from "@tanstack/solid-router";
import { HydrationScript } from "solid-js/web";
import indexCss from "../index.css?url";

export const Route = createRootRoute({
  head: () => ({
    meta: [
      { charSet: "utf-8" },
      { name: "viewport", content: "width=device-width, initial-scale=1" },
      { title: "Visual review" },
    ],
    // Stylesheets go through `head` rather than a `<link>` in the shell below:
    // the shell renders server-side only, so an asset referenced from it is
    // never emitted into the client build. A build appends StyleX's compiled
    // rules to `index.css`; in dev they come from the plugin's virtual
    // stylesheet instead, which `StyleXDevRuntime` keeps up to date.
    links: [
      { rel: "stylesheet", href: indexCss },
      ...(import.meta.env.DEV ? [{ rel: "stylesheet", href: "/virtual:stylex.css" }] : []),
    ],
  }),
  shellComponent: RootComponent,
});

function RootComponent() {
  return (
    <html lang="en">
      <head>
        <HydrationScript />
        <HeadContent />
      </head>
      <body>
        <Outlet />
        <Scripts />
      </body>
    </html>
  );
}

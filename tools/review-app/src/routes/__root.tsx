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
    // never emitted into the client build. `index.css` is the whole stylesheet
    // in both modes — Panda's PostCSS plugin substitutes the generated rules
    // into it as Vite processes it, so there is no dev-only half to link.
    links: [{ rel: "stylesheet", href: indexCss }],
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

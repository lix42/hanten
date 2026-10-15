import { createFileRoute } from "@tanstack/solid-router";
import { LabelApp } from "../LabelApp";
import { getLabelSession } from "../server/labels";

export const Route = createFileRoute("/label")({
  head: () => ({ meta: [{ title: "Chart labels" }] }),
  loader: () => getLabelSession(),
  component: LabelRoute,
});

function LabelRoute() {
  const session = Route.useLoaderData();
  return <LabelApp session={session()} />;
}

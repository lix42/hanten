import { createFileRoute } from "@tanstack/solid-router";
import { App } from "../App";
import { getReviewSet } from "../server/getReviewSet";
import { SetError } from "../SetError";
import { StyleXDevRuntime } from "../StyleXDevRuntime";

export const Route = createFileRoute("/")({
  loader: () => getReviewSet(),
  component: IndexRoute,
  errorComponent: (props) => <SetError error={props.error} />,
});

function IndexRoute() {
  const set = Route.useLoaderData();
  return (
    <>
      <StyleXDevRuntime />
      <App set={set()} />
    </>
  );
}

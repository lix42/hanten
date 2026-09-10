import { createFileRoute } from "@tanstack/solid-router";
import { App } from "../App";
import { StyleXDevRuntime } from "../StyleXDevRuntime";

export const Route = createFileRoute("/")({ component: IndexRoute });

function IndexRoute() {
  return (
    <>
      <StyleXDevRuntime />
      <App />
    </>
  );
}

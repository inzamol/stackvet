import { Button } from "@myorg/shared-ui";
import { throttle } from "lodash";

export function Admin() {
  throttle(() => {}, 100);
  return Button();
}

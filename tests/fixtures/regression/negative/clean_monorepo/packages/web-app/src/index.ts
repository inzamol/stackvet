import { Button } from "@myorg/shared-ui";
import { debounce } from "lodash";

export function App() {
  debounce(() => {}, 100);
  return Button();
}

import { bindFilterEvents } from "./event-bindings/filter-events";
import { bindModalActionEvents } from "./event-bindings/modal-actions";
import { bindNavigationAndServiceEvents } from "./event-bindings/navigation-service";

export function bindMainEvents(context) {
  bindNavigationAndServiceEvents(context);
  bindModalActionEvents(context);
  bindFilterEvents(context);
}

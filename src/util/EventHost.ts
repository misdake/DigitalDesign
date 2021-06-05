import {Event} from "./Events";

export type EventCallback<T, Params> = (obj: T, params: Params) => void;

export class EventHost {

    private events: Map<Event<any, any>, Map<any, EventCallback<any, any>>>;

    constructor() {
        this.events = new Map<Event<any, any>, Map<any, EventCallback<any, any>>>();
    }

    fire<T, Params>(event: Event<T, Params>, obj: T, params?: Params) {
        this.events.get(event)?.forEach((value) => value(obj, params));
    }

    on<Self, T, Params>(event: Event<T, Params>, self: Self, callback: EventCallback<T, Params>, allowReplace: boolean = false, log: boolean = false) {
        let map = this.events.get(event);
        if (!map) {
            this.events.set(event, map = new Map<any, EventCallback<any, any>>());
        }

        if (log) {
            let oldCallback = callback;
            callback = (obj, params) => {
                console.log(`event '${event.name}' fired\nfrom`, this, `\nto`, obj, `\nparams`, params);
                oldCallback(obj, params);
            };
        }

        let cb = map.get(self);
        if (!cb || allowReplace) {
            //set or replace
            map.set(self, callback);
        } else if (!cb && !allowReplace) {
            //trying to replace && !allowReplace => fail
            console.log("trying to replace && !allowReplace: event =", name, self);
            debugger;
            map.set(self, callback);
        }
    }

    off<Self, T, Params>(event : Event<T, Params>, self?: Self): boolean {
        let map = this.events.get(event);
        if (map && map.has(self)) {
            return map.delete(self);
        }
        return false;
    }

}
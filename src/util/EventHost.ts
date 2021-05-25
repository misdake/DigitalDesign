export type EventCallback<T> = (obj: T, ...params: any[]) => void;

export class EventHost {

    private events: Map<string, Map<any, EventCallback<any>>>;

    constructor() {
        this.events = new Map<string, Map<any, EventCallback<any>>>();
    }

    fire(name: string, ...params: any[]) {
        this.events.get(name)?.forEach((value, key) => {
            value(key, ...params);
        });
    }

    on<T>(name: string, obj: T, callback: EventCallback<T>, allowReplace: boolean = false, log: boolean = false) {
        let map = this.events.get(name);
        if (!map) {
            this.events.set(name, map = new Map<any, EventCallback<any>>());
        }

        if (log) {
            let oldCallback = callback;
            callback = (obj, ...params) => {
                console.log(`event '${name}' fired\nfrom`, this, `\nto`, obj, `\nparams`, params);
                oldCallback(obj, ...params);
            };
        }

        let cb = map.get(obj);
        if (!cb || allowReplace) {
            //set or replace
            map.set(obj, callback);
        } else if (!cb && !allowReplace) {
            //trying to replace && !allowReplace => fail
            console.log("trying to replace && !allowReplace: event =", name, obj);
            debugger;
            map.set(obj, callback);
        }
    }

    off<T>(name: string, obj?: T): boolean {
        let map = this.events.get(name);
        if (map && map.has(obj)) {
            return map.delete(obj);
        }
        return false;
    }

}
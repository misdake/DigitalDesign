import {GameComp} from "../game/GameComp";
import {GameWire} from "../game/GameWire";

export class Event<T, Params> {
    public readonly name: string;
    constructor(name: string) {
        this.name = name;
    }
}

export namespace Events {
    export const COMPONENT_ADD = new Event<GameComp, void>("COMPONENT_ADD");
    export const COMPONENT_REMOVE = new Event<GameComp, void>("COMPONENT_REMOVE");
    export const COMPONENT_UPDATE = new Event<GameComp, { x: number, y: number }>("COMPONENT_UPDATE");
    export const WIRE_ADD = new Event<GameWire, void>("WIRE_ADD");
    export const WIRE_REMOVE = new Event<GameWire, void>("WIRE_REMOVE");
    export const WIRE_UPDATE = new Event<GameWire, void>("WIRE_UPDATE");
}
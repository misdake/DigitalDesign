import {GameComp} from "../game/GameComp";
import {GameWire} from "../game/GameWire";
import {WireElement} from "../ui/component/WireElement";
import {CompElement} from "../ui/component/CompElement";
import {InputPinElement, OutputPinElement} from "../ui/component/PinElement";

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
    export const WIRES_REMOVE = new Event<GameWire[], void>("WIRES_REMOVE");
    export const WIRE_UPDATE = new Event<GameWire, void>("WIRE_UPDATE");

    export const COMPONENT_UI_CREATED = new Event<CompElement, void>("COMPONENT_UI_CREATED");
    export const INPUTPIN_UI_CREATED = new Event<InputPinElement, void>("INPUTPIN_UI_CREATED");
    export const OUTPUTPIN_UI_CREATED = new Event<OutputPinElement, void>("OUTPUTPIN_UI_CREATED");
    export const WIRE_UI_CREATED = new Event<WireElement, void>("WIRE_UI_CREATED");
}
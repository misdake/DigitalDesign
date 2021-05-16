import {Wire} from "../logic/Component";
import {GamePin} from "./GamePin";

export class GameWire {
    readonly wire: Wire;

    readonly fromPin: GamePin;
    readonly toPin: GamePin;

    constructor(wire: Wire, fromPin: GamePin, toPin: GamePin) {
        this.wire = wire;
        this.fromPin = fromPin;
        this.toPin = toPin;
    }
}
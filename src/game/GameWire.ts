import {Wire} from "../logic/Component";
import {GamePin} from "./GamePin";
import {EventHost} from "../util/EventHost";

export class GameWire extends EventHost {
    readonly wire: Wire;

    readonly fromPin: GamePin;
    readonly toPin: GamePin;

    constructor(wire: Wire, fromPin: GamePin, toPin: GamePin) {
        super();
        this.wire = wire;
        this.fromPin = fromPin;
        this.toPin = toPin;

        fromPin.gameComp.on("move", this, () => {
            this.fire("render");
        });
        toPin.gameComp.on("move", this, () => {
            this.fire("render");
        });
    }
}
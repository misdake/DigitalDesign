import {Wire} from "../logic/Component";
import {GamePin} from "./GamePin";
import {EventHost} from "../util/EventHost";
import {Events} from "../util/Events";

export class GameWire extends EventHost {
    readonly wire: Wire;

    readonly fromPin: GamePin;
    readonly toPin: GamePin;

    constructor(wire: Wire, fromPin: GamePin, toPin: GamePin) {
        super();
        this.wire = wire;
        this.fromPin = fromPin;
        this.toPin = toPin;

        fromPin.gameComp.on(Events.COMPONENT_UPDATE, this, () => {
            this.fire(Events.WIRE_UPDATE, this);
        });
        toPin.gameComp.on(Events.COMPONENT_UPDATE, this, () => {
            this.fire(Events.WIRE_UPDATE, this);
        });
    }
}
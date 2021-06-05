import {Game} from "../Game";
import {Editor} from "../Editor";
import {InputPinElement, OutputPinElement} from "../../ui/component/PinElement";
import {GameWire} from "../GameWire";
import {Wire} from "../../logic/Component";
import {Events} from "../../util/Events";

export class EditorWire {
    private game: Game;
    private editor: Editor;
    constructor(game: Game, editor: Editor) {
        this.game = game;
        this.editor = editor;
    }

    createWire(from: OutputPinElement, to: InputPinElement) {
        let wire = new Wire("wire", from.gameComp.component, from.gamePin.pin, to.gameComp.component, to.gamePin.pin);
        let gameWire = new GameWire(wire, from.gamePin, to.gamePin);
        this.game.wires.push(gameWire);
        this.game.fire(Events.WIRE_ADD, gameWire);
    }

    removeWire(gameWire: GameWire) {
        const index = this.game.wires.indexOf(gameWire);
        if (index > -1) {
            this.game.wires.splice(index, 1);
            this.game.fire(Events.WIRE_REMOVE, gameWire);
            return true;
        }
        return false;
    }
}
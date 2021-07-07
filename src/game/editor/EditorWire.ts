import {Game} from "../Game";
import {Editor} from "../Editor";
import {InputPinElement, OutputPinElement} from "../../ui/component/PinElement";
import {GameWire} from "../GameWire";
import {Wire} from "../../logic/Component";
import {Events} from "../../util/Events";
import {GameComp} from "../GameComp";

function filterInPlace<T>(array: T[], condition: (t: T, index: number, array: T[]) => boolean) {
    let i = 0, j = 0;

    while (i < array.length) {
        const val = array[i];
        if (condition(val, i, array)) array[j++] = val;
        i++;
    }

    array.length = j;
    return array;
}

export class EditorWire {
    private game: Game;
    private editor: Editor;
    constructor(game: Game, editor: Editor) {
        this.game = game;
        this.editor = editor;
    }

    createWire(from: OutputPinElement, to: InputPinElement) {
        let wire = new Wire(from.gameComp.component, from.gamePin.pin, to.gameComp.component, to.gamePin.pin);
        let gameWire = new GameWire(wire, from.gamePin, to.gamePin);
        this.game.wires.push(gameWire);
        this.game.fire(Events.WIRE_ADD, gameWire);

        this.game._editMain_editor("create wire", main => {
            main.wires.push(gameWire.wire);
        });
    }

    removeWiresOfCompoment(gameComp: GameComp) {
        filterInPlace(this.game.wires, (wire, i) => {
            let toRemove = wire.fromPin.gameComp === gameComp || wire.toPin.gameComp === gameComp;
            return !toRemove;
        });
        this.game._editMain_editor("remove wires", main => {
            main.wires = main.wires.filter((wire, i) => {
                let toRemove = wire.fromComponent === gameComp.component || wire.toComponent === gameComp.component;
                return !toRemove;
            });
        });
    }

    removeWire(gameWire: GameWire, index: number = -1) {
        if (!(index >= 0 && this.game.wires[index] === gameWire)) {
            index = this.game.wires.indexOf(gameWire);
        }

        if (index > -1) {
            this.game.wires.splice(index, 1);
            this.game.fire(Events.WIRE_REMOVE, gameWire);

            this.game._editMain_editor("remove wire", main => {
                main.wires.splice(index, 1);
            });
            return true;
        }
        return false;
    }
}
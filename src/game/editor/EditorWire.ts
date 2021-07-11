import {Game} from "../Game";
import {Editor} from "../Editor";
import {InputPinElement, OutputPinElement} from "../../ui/component/PinElement";
import {GameWire} from "../GameWire";
import {Wire} from "../../logic/Component";
import {Events} from "../../util/Events";
import {GameComp} from "../GameComp";
import {GamePin} from "../GamePin";

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
        this.removeWiresOfPin(to.gamePin);

        let wire = new Wire(from.gameComp.component, from.gamePin.pin, to.gameComp.component, to.gamePin.pin);
        let gameWire = new GameWire(wire, from.gamePin, to.gamePin);
        this.game.wires.push(gameWire);
        this.game.fire(Events.WIRE_ADD, gameWire);

        this.game._editMain_editor("create wire", main => {
            main.wires.push(gameWire.wire);
        });
    }

    removeWires(filter: (wire: GameWire) => boolean) : number {
        let removed: GameWire[] = [];
        let wireSet: Set<Wire> = new Set<Wire>();
        filterInPlace(this.game.wires, wire => {
            let toRemove = filter(wire);
            if (toRemove) {
                removed.push(wire);
                wireSet.add(wire.wire);
            }
            return !toRemove;
        });
        if (removed.length) {
            this.game.fire(Events.WIRES_REMOVE, removed);
            this.game._editMain_editor("remove wires", main => {
                main.wires = main.wires.filter(wire => !wireSet.has(wire));
            });
        }
        return removed.length;
    }

    removeWiresOfPin(gamePin: GamePin) : number {
        return this.removeWires(wire => wire.fromPin === gamePin || wire.toPin === gamePin);
    }

    removeWiresOfCompoment(gameComp: GameComp) : number {
        return this.removeWires(wire => wire.fromPin.gameComp === gameComp || wire.toPin.gameComp === gameComp);
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
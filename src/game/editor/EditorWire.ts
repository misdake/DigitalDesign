import {Game} from "../Game";
import {Editor} from "../Editor";
import {InputPinElement, OutputPinElement} from "../../ui/component/PinElement";
import {GameWire} from "../GameWire";

export class EditorWire {
    private game: Game;
    private editor: Editor;
    constructor(game: Game, editor: Editor) {
        this.game = game;
        this.editor = editor;
    }

    createWire(gameWire: GameWire) {
        //TODO gameWire改为在这个方法里创建

        this.game.wires.push(gameWire);
        //TODO 刷新UI
    }

    removeWire(gameWire: GameWire) {
        const index = this.game.wires.indexOf(gameWire);
        if (index > -1) {
            this.game.wires.splice(index, 1);

            this.editor.doUpdate(); //TODO 指明类型

            return true;
        }
        return false;
    }
}
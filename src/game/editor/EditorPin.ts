import {Game} from "../Game";
import {Editor} from "../Editor";
import {InputPinElement, OutputPinElement} from "../../ui/component/PinElement";

export class EditorPin {
    private game: Game;
    private editor: Editor;
    constructor(game: Game, editor: Editor) {
        this.game = game;
        this.editor = editor;
    }

    private selectedPin: InputPinElement | OutputPinElement = null;
    isSelectedPin(element: InputPinElement | OutputPinElement) {
        return element === this.selectedPin;
    }
    deselectPin() {
        this.selectedPin = null;
        //TODO trigger ui update
    }
    selectInputPin(inputPin: InputPinElement) {
        let oldSelected = this.selectedPin;

        if (this.selectedPin) {
            if (this.selectedPin === inputPin) {
                this.selectedPin = null;
            } else if (this.selectedPin instanceof OutputPinElement) {
                let outputPin = this.selectedPin;
                this.selectedPin = null;
                this.tryConnect(outputPin, inputPin);
            } else if (this.selectedPin instanceof InputPinElement) {
                this.selectedPin = inputPin;
            }
        } else {
            this.selectedPin = inputPin;
        }
        if (oldSelected !== this.selectedPin) {
            oldSelected?.requestUpdate();
            this.selectedPin?.requestUpdate();
        }
    }
    selectOutputPin(outputPin: OutputPinElement) {
        let oldSelected = this.selectedPin;

        if (this.selectedPin) {
            if (this.selectedPin === outputPin) {
                this.selectedPin = null;
            } else if (this.selectedPin instanceof InputPinElement) {
                let inputPin = this.selectedPin;
                this.selectedPin = null;
                this.tryConnect(outputPin, inputPin);
            } else if (this.selectedPin instanceof OutputPinElement) {
                this.selectedPin = outputPin;
            }
        } else {
            this.selectedPin = outputPin;
        }
        if (oldSelected !== this.selectedPin) {
            oldSelected?.requestUpdate();
            this.selectedPin?.requestUpdate();
        }
    }

    private tryConnect(from: OutputPinElement, to: InputPinElement): boolean {
        //不同的component
        if (from.gameComp === to.gameComp) return false;

        //TODO 也不能构成环

        //剩下情况都可以连接
        this.editor.wire.createWire(from, to); //TODO 这里面请求了更新，是不是外面就不要更新了？

        return true;
    }
}
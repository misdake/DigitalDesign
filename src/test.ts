import {System} from "./System";
import {ComponentInput} from "./ComponentLib";

let system = new System();

let c1a = system.inputComponents[0] as ComponentInput;
let c1b = system.inputComponents[1] as ComponentInput;

c1a.data = 0;
c1b.data = 0;
console.log("run1:");
system.run();
console.log("");

c1a.data = 1;
c1b.data = 0;
console.log("run2:");
system.run();
console.log("");

c1a.data = 0;
c1b.data = 1;
console.log("run3:");
system.run();
console.log("");

c1a.data = 1;
c1b.data = 1;
console.log("run4:");
system.run();
console.log("");
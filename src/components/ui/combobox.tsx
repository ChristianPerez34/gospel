import { Combobox as ComboboxPrimitive } from "@base-ui/react/combobox";
import { Check, Search } from "lucide-react";
import { Input } from "./input";

// shadcn's Base UI composition, styled with Gospel's shared surface tokens.
export const Combobox = ComboboxPrimitive.Root;
export const ComboboxLabel = ComboboxPrimitive.Label;
export const ComboboxTrigger = ComboboxPrimitive.Trigger;

export function ComboboxInput(props: ComboboxPrimitive.Input.Props) {
  return (
    <div className="combobox-search">
      <Search size={15} aria-hidden="true" />
      <ComboboxPrimitive.Input render={<Input />} {...props} />
    </div>
  );
}
export function ComboboxContent(props: ComboboxPrimitive.Popup.Props) {
  return (
    <ComboboxPrimitive.Portal>
      <ComboboxPrimitive.Positioner
        sideOffset={6}
        align="start"
        collisionAvoidance={{ side: "none", align: "shift" }}
        className="combobox-positioner"
      >
        <ComboboxPrimitive.Popup
          data-slot="combobox-content"
          className="combobox-content"
          {...props}
        />
      </ComboboxPrimitive.Positioner>
    </ComboboxPrimitive.Portal>
  );
}
export function ComboboxList(props: ComboboxPrimitive.List.Props) {
  return <ComboboxPrimitive.List data-slot="combobox-list" className="combobox-list" {...props} />;
}
export function ComboboxItem({ children, ...props }: ComboboxPrimitive.Item.Props) {
  return (
    <ComboboxPrimitive.Item data-slot="combobox-item" className="combobox-item" {...props}>
      {children}
      <ComboboxPrimitive.ItemIndicator className="combobox-item-check">
        <Check size={15} aria-hidden="true" />
      </ComboboxPrimitive.ItemIndicator>
    </ComboboxPrimitive.Item>
  );
}
export function ComboboxEmpty(props: ComboboxPrimitive.Empty.Props) {
  return (
    <ComboboxPrimitive.Empty data-slot="combobox-empty" className="combobox-empty" {...props} />
  );
}

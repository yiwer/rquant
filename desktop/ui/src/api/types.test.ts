import type { BookCardDto } from "@bindings/BookCardDto";

test("bindings types are importable", () => {
  const card: BookCardDto = {
    book: "b1", title: "t", kind: "single", status: "empty",
    advice: null, nav: null, total_return: null, max_drawdown: null,
    pos: null, state_time: null, holdings: null, last_signal: null,
  };
  expect(card.book).toBe("b1");
});

defmodule MorrowComparison do
  @moduledoc """
  Elixir equivalents of workloads.mr and workloads.rs.

  Compile once with elixirc, then invoke MorrowComparison.main/0 with runtime
  arguments. The default model uses a list of {index, value} tuples; model-struct
  uses named structs. Both allocate a new list on every update, retain unchanged
  cells, and keep the original model available until after the final checksum.

  `warm MODE STEPS SEED ROUNDS` emits an untallied warmup at round zero followed
  by ROUNDS samples in one VM. Timings cover model construction, updates and final
  checksums, but exclude the independent oracle, validation and output. There is
  no forced collection between rounds; ordinary BEAM garbage collection remains
  part of the measured workload. Native compilation and VM boot are excluded.
  """

  defmodule Cell do
    defstruct [:index, :value]
  end

  @modes ~w(scalar model model-struct precision)

  def main(args \\ System.argv()) do
    case args do
      ["verify"] ->
        verify()

      ["batch", mode, steps, seed, repeats] when mode in @modes ->
        steps = decimal!(steps, 1_000_000_000)
        seed = decimal!(seed, 2_147_483_646)
        repeats = decimal!(repeats, 1_000)
        batch(mode, steps, seed, repeats)

      ["warm", mode, steps, seed, rounds] when mode in @modes ->
        warm(
          mode,
          decimal!(steps, 1_000_000_000),
          decimal!(seed, 2_147_483_646),
          decimal!(rounds, 1_000)
        )

      [mode, steps, seed] when mode in @modes ->
        print_result(
          mode,
          run(mode, decimal!(steps, 1_000_000_000), decimal!(seed, 2_147_483_646))
        )

      _ ->
        raise ArgumentError,
              "expected verify, or [warm|batch] scalar|model|model-struct|precision STEPS SEED [ROUNDS]"
    end
  end

  defp batch(_mode, _steps, _seed, 0), do: :ok

  defp batch(mode, steps, seed, repeats) do
    print_result(mode, run(mode, steps, seed))
    batch(mode, steps, seed, repeats - 1)
  end

  defp print_result(mode, {checksum, original}) do
    IO.puts(checksum)
    if mode in ["model", "model-struct"], do: IO.puts(original)
  end

  defp verify do
    IO.puts("case,mode,steps,seed,result,original")

    cases =
      for mode <- @modes,
          steps <- [0, 1, 2, 255, 256, 257, 1_000],
          seed <- [0, 1, 7, 17, 255, 256, 2_147_483_646],
          do: {mode, steps, seed}

    cases
    |> Enum.with_index()
    |> Enum.each(fn {{mode, steps, seed}, index} ->
      result = run(mode, steps, seed)
      expected = oracle(mode, steps, seed)

      unless result == expected do
        raise "incorrect #{mode} output: #{inspect(result)}; expected #{inspect(expected)}"
      end

      {checksum, original} = result
      IO.puts("#{index},#{mode},#{steps},#{seed},#{checksum},#{original}")
    end)
  end

  def run("scalar", steps, seed), do: {scalar(steps, seed), 0}
  def run("precision", _steps, seed), do: {9_007_199_254_740_993 + seed, 0}

  def run("model", steps, seed) do
    original = Enum.map(0..255, &{&1, 0})
    result = model(steps, 0, seed, original)

    {Enum.reduce(result, 0, fn {index, value}, sum -> sum + (index + 1) * value end),
     Enum.reduce(original, 0, fn {_, value}, sum -> sum + value end)}
  end

  def run("model-struct", steps, seed) do
    original = Enum.map(0..255, &%Cell{index: &1, value: 0})
    result = model_struct(steps, 0, seed, original)

    {Enum.reduce(result, 0, fn cell, sum -> sum + (cell.index + 1) * cell.value end),
     Enum.reduce(original, 0, fn cell, sum -> sum + cell.value end)}
  end

  defp scalar(0, value), do: value
  defp scalar(remaining, value), do: scalar(remaining - 1, rem(value * 48_271, 2_147_483_647))

  defp model(0, _step, _seed, values), do: values

  defp model(remaining, step, seed, values) do
    target = rem(seed + step * 17, 256)

    updated =
      Enum.map(values, fn {index, value} = cell ->
        if index == target, do: {index, value + 1}, else: cell
      end)

    model(remaining - 1, step + 1, seed, updated)
  end

  defp model_struct(0, _step, _seed, values), do: values

  defp model_struct(remaining, step, seed, values) do
    target = rem(seed + step * 17, 256)

    updated =
      Enum.map(values, fn cell ->
        if cell.index == target, do: %{cell | value: cell.value + 1}, else: cell
      end)

    model_struct(remaining - 1, step + 1, seed, updated)
  end

  defp warm(mode, steps, seed, rounds) do
    expected = oracle(mode, steps, seed)
    IO.puts("round,elapsed_ns,checksum,original")

    Enum.each(0..rounds, fn round ->
      start = System.monotonic_time()
      result = run(mode, steps, seed)
      elapsed = System.monotonic_time() - start

      unless result == expected do
        raise "incorrect #{mode} output: #{inspect(result)}; expected #{inspect(expected)}"
      end

      {checksum, original} = result
      ns = System.convert_time_unit(elapsed, :native, :nanosecond)
      IO.puts("#{round},#{ns},#{checksum},#{original}")
    end)
  end

  # Independent closed-form checks, outside the timing interval. The inverse of
  # 17 modulo 256 is 241, so each cell's first update and update count are known.
  defp oracle("scalar", steps, seed),
    do: {rem(seed * modular_power(48_271, steps, 1), 2_147_483_647), 0}

  defp oracle("precision", _steps, seed), do: {9_007_199_254_740_993 + seed, 0}

  defp oracle(mode, steps, seed) when mode in ["model", "model-struct"] do
    checksum =
      Enum.reduce(0..255, 0, fn index, sum ->
        first = rem((index + 256 - rem(seed, 256)) * 241, 256)
        count = if steps <= first, do: 0, else: 1 + div(steps - 1 - first, 256)
        sum + (index + 1) * count
      end)

    {checksum, 0}
  end

  defp modular_power(_base, 0, result), do: result

  defp modular_power(base, exponent, result) do
    result = if rem(exponent, 2) == 1, do: rem(result * base, 2_147_483_647), else: result
    modular_power(rem(base * base, 2_147_483_647), div(exponent, 2), result)
  end

  defp decimal!(text, maximum) do
    case Integer.parse(text) do
      {value, ""} when value >= 0 and value <= maximum -> value
      _ -> raise ArgumentError, "expected integer in 0..#{maximum}, got #{inspect(text)}"
    end
  end
end

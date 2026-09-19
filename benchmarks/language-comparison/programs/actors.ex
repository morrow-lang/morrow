defmodule ActorComparison do
  @moduledoc "Matched BEAM implementations for the bounded actor comparison."

  def main(args \\ System.argv()) do
    case args do
      ["request-reply", clients, requests] ->
        request_reply(decimal!(clients, 1_000), decimal!(requests, 1_000_000))

      ["contention", schedulers, probes, work] ->
        contention(
          decimal!(schedulers, 64),
          decimal!(probes, 10_000),
          decimal!(work, 1_000_000_000)
        )

      ["lifecycle", workers, faults] ->
        lifecycle(decimal!(workers, 1_000), decimal!(faults, 500))

      _ ->
        raise ArgumentError,
              "expected request-reply CLIENTS REQUESTS, contention SCHEDULERS PROBES WORK, or lifecycle WORKERS FAULTS"
    end
  end

  defp request_reply(clients, requests) when clients > 0 and requests > 0 do
    IO.puts("ready")
    parent = self()
    collector = spawn(fn -> request_collector(clients, clients, requests, 0, 0, parent) end)
    server = spawn(fn -> request_server(clients * requests) end)

    Enum.each(0..(clients - 1), fn id ->
      client = spawn(fn -> request_client(server, collector, id, requests) end)
      send(client, {:start, client})
    end)

    receive do
      {:finished, line} -> IO.puts(line)
    end
  end

  defp request_server(remaining) do
    receive do
      {:ask, value, reply} ->
        send(reply, {:answer, value * 3 + 1})
        if remaining > 1, do: request_server(remaining - 1)
    end
  end

  defp request_client(server, collector, id, requests) do
    receive do
      {:start, reply} -> request_loop(server, collector, reply, id, requests, 0, 0)
    end
  end

  defp request_loop(_server, collector, _reply, _id, requests, requests, checksum) do
    send(collector, {:request_done, requests, checksum})
  end

  defp request_loop(server, collector, reply, id, requests, index, checksum) do
    send(server, {:ask, id * 100_000 + index, reply})

    receive do
      {:answer, value} ->
        request_loop(server, collector, reply, id, requests, index + 1, checksum + value)
    end
  end

  defp request_collector(1, clients, requests, replies, checksum, parent) do
    receive do
      {:request_done, count, value} ->
        send(
          parent,
          {:finished,
           "request-reply,#{clients},#{requests},#{replies + count},#{checksum + value}"}
        )
    end
  end

  defp request_collector(remaining, clients, requests, replies, checksum, parent) do
    receive do
      {:request_done, count, value} ->
        request_collector(
          remaining - 1,
          clients,
          requests,
          replies + count,
          checksum + value,
          parent
        )
    end
  end

  defp contention(schedulers, probes, work)
       when schedulers > 0 and probes > 0 and work >= 0 do
    IO.puts("ready")
    parent = self()
    collector = spawn(fn -> contention_collector(2, probes, work, 0, 0, parent) end)
    server = spawn(fn -> probe_server(probes) end)
    spawn_dummies(schedulers - 1)
    spawn(fn -> hot_loop(work, 7, collector) end)
    client = spawn(fn -> probe_client(server, collector, probes) end)
    send(client, {:start_probe, client})

    receive do
      {:finished, line} -> IO.puts(line)
    end
  end

  defp spawn_dummies(0), do: :ok

  defp spawn_dummies(remaining) do
    spawn(fn -> :ok end)
    spawn_dummies(remaining - 1)
  end

  defp probe_server(remaining) do
    receive do
      {:ping, value, reply} ->
        send(reply, {:pong, value * 2 + 1})
        if remaining > 1, do: probe_server(remaining - 1)
    end
  end

  defp probe_client(server, collector, probes) do
    receive do
      {:start_probe, reply} -> probe_loop(server, collector, reply, probes, 0, 0)
    end
  end

  defp probe_loop(_server, collector, _reply, probes, probes, checksum) do
    send(collector, {:probe_done, checksum})
  end

  defp probe_loop(server, collector, reply, probes, index, checksum) do
    send(server, {:ping, index, reply})

    receive do
      {:pong, value} ->
        IO.puts("sample,#{index}")
        probe_loop(server, collector, reply, probes, index + 1, checksum + value)
    end
  end

  defp hot_loop(0, value, collector) do
    IO.puts("hot-done")
    send(collector, {:hot_done, value})
  end

  defp hot_loop(remaining, value, collector) do
    hot_loop(remaining - 1, rem(value * 48_271, 2_147_483_647), collector)
  end

  defp contention_collector(remaining, probes, work, probe_checksum, hot_checksum, parent) do
    receive do
      {:probe_done, value} ->
        finish_or_collect(
          remaining,
          probes,
          work,
          value,
          hot_checksum,
          parent
        )

      {:hot_done, value} ->
        finish_or_collect(
          remaining,
          probes,
          work,
          probe_checksum,
          value,
          parent
        )
    end
  end

  defp finish_or_collect(1, probes, work, probe_checksum, hot_checksum, parent) do
    send(parent, {:finished, "contention,#{probes},#{work},#{probe_checksum},#{hot_checksum}"})
  end

  defp finish_or_collect(remaining, probes, work, probe_checksum, hot_checksum, parent) do
    contention_collector(
      remaining - 1,
      probes,
      work,
      probe_checksum,
      hot_checksum,
      parent
    )
  end

  defp lifecycle(workers, faults) when workers > 0 and faults >= 0 do
    IO.puts("ready")
    parent = self()

    collector =
      spawn(fn ->
        lifecycle_collector(
          workers + faults * 2,
          workers,
          faults,
          0,
          0,
          0,
          0,
          parent
        )
      end)

    Enum.each(0..(workers - 1), fn id ->
      spawn(fn -> send(collector, {:life_done, id * 17 + 1}) end)
    end)

    if faults > 0 do
      Enum.each(0..(faults - 1), fn id ->
        spawn(fn -> fault_supervisor(id, collector, parent, 1) end)
      end)
    end

    receive do
      {:finished, line} -> IO.puts(line)
    end

    wait_for_faults(faults)
  end

  defp wait_for_faults(0), do: :ok

  defp wait_for_faults(remaining) do
    receive do
      :fault_complete -> wait_for_faults(remaining - 1)
    end
  end

  defp fault_supervisor(id, collector, parent, restarts) do
    {_pid, reference} =
      spawn_monitor(fn ->
        send(collector, {:attempt, id})
        exit(:controlled_fault)
      end)

    receive do
      {:DOWN, ^reference, :process, _pid, :controlled_fault} ->
        if restarts > 0 do
          fault_supervisor(id, collector, parent, restarts - 1)
        else
          send(parent, :fault_complete)
        end
    end
  end

  defp lifecycle_collector(
         1,
         workers,
         faults,
         completed,
         attempts,
         worker_checksum,
         fault_checksum,
         parent
       ) do
    receive do
      message ->
        {completed, attempts, worker_checksum, fault_checksum} =
          lifecycle_update(message, completed, attempts, worker_checksum, fault_checksum)

        send(
          parent,
          {:finished,
           "lifecycle,#{workers},#{faults},#{completed},#{attempts},#{worker_checksum},#{fault_checksum}"}
        )
    end
  end

  defp lifecycle_collector(
         remaining,
         workers,
         faults,
         completed,
         attempts,
         worker_checksum,
         fault_checksum,
         parent
       ) do
    receive do
      message ->
        {completed, attempts, worker_checksum, fault_checksum} =
          lifecycle_update(message, completed, attempts, worker_checksum, fault_checksum)

        lifecycle_collector(
          remaining - 1,
          workers,
          faults,
          completed,
          attempts,
          worker_checksum,
          fault_checksum,
          parent
        )
    end
  end

  defp lifecycle_update({:life_done, value}, completed, attempts, worker_checksum, fault_checksum),
    do: {completed + 1, attempts, worker_checksum + value, fault_checksum}

  defp lifecycle_update({:attempt, id}, completed, attempts, worker_checksum, fault_checksum),
    do: {completed, attempts + 1, worker_checksum, fault_checksum + id}

  defp decimal!(text, maximum) do
    case Integer.parse(text) do
      {value, ""} when value >= 0 and value <= maximum -> value
      _ -> raise ArgumentError, "expected integer in 0..#{maximum}, got #{inspect(text)}"
    end
  end
end

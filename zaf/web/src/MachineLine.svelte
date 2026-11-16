<script>
  import { machineLine } from './api.js';

  let { machine } = $props();
</script>

<p class="machine-line" data-testid="machine-line">{machineLine(machine)}</p>

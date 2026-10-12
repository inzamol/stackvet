from celery import shared_task

# CEL-003: Task missing retry configuration (autoretry_for)
# CEL-004: Task missing time_limit or soft_time_limit
@shared_task
def process_data(item_id):
    return item_id * 2

def trigger():
    # CEL-001: Task executed synchronously with .apply()
    process_data.apply(args=[1])
